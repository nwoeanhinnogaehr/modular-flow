use super::graph::*;
use std::sync::Arc;
use std::mem;
use std::slice;
use std::marker::PhantomData;
use std::ops::Deref;
use std::thread;

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext {
    id: NodeID,
    sched: Arc<Scheduler>,
}

// TODO T must be copy!
pub struct Data<'a, T: 'a> {
    data: Vec<T>,
    // as a marker that data will eventually be made a slice
    phantom: PhantomData<&'a T>,

    // to inform the writer that this has been dropped
    drop_sig: Arc<CondvarCell<bool>>,
}

impl<'a, T: 'a> Data<'a, T> {
    pub fn new(data: Vec<T>, drop_sig: Arc<CondvarCell<bool>>) -> Data<'a, T> {
        Data {
            data,
            phantom: Default::default(),
            drop_sig,
        }
    }
}

impl<'a, T: 'a> Drop for Data<'a, T> {
    fn drop(&mut self) {
        let lock = self.drop_sig.value.lock().unwrap();
        lock.set(true);
        self.drop_sig.cond.notify_one();
    }
}

impl<'a, T: 'a> Deref for Data<'a, T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl NodeContext {
    /// Block until at least one byte is available
    pub fn read_any<T: Copy>(&self, port: InPortID) -> Data<T> {
        self.read(port, ReadRequest::Any)
    }
    pub fn read_n<T: Copy>(&self, port: InPortID, n: usize) -> Data<T> {
        self.read(port, ReadRequest::N(n))
    }
    fn read<'a, T: Copy>(&'a self, port: InPortID, req: ReadRequest) -> Data<'a, T> {
        self.sched.read(self.id, port, req)
    }
    pub fn write<T: Copy>(&self, port: OutPortID, src: &[T]) {
        self.sched.write(self.id, port, src);
    }
}

impl Drop for NodeContext {
    fn drop(&mut self) {
        self.sched.graph.detach_thread(self.id).unwrap();
    }
}

/**
 * The supervisor runs the main loop. It owns the graph, and manages access to the graph while the
 * application is running. Node threads must be attached to the supervisor by requesting a context.
 */
pub struct Supervisor {
    sched: Arc<Scheduler>,
}

impl Supervisor {
    pub fn new(graph: Graph) -> Supervisor {
        Supervisor {
            sched: Arc::new(Scheduler::new(graph)),
        }
    }
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext, ()> {
        self.sched.graph.attach_thread(node).map(|_| {
            NodeContext {
                sched: self.sched.clone(),
                id: node,
            }
        })
    }
    pub fn run(self) {
        thread::park();
    }
}

pub struct Scheduler {
    graph: Graph,
}


impl Scheduler {
    fn new(graph: Graph) -> Scheduler {
        Scheduler { graph }
    }
    // TODO T must be copy
    fn write<'a, T>(&self, node: NodeID, port: OutPortID, data: &[T]) {
        let data_bytes: &[u8] = unsafe {
            slice::from_raw_parts(
                mem::transmute(data.as_ptr()),
                data.len() * mem::size_of::<T>(),
            )
        };

        let out_node = self.graph.node(node);
        let out_port = out_node.out_port(port);

        let in_edge = out_port.edge.unwrap();
        let in_node = self.graph.node(in_edge.node);
        let in_port: &InPort = in_node.in_port(in_edge.port);


        // append data to buffer and get number of elements available

        // TODO while there is enough data for the reader to use
        // synchronize data exchange with reader

        {
            let data = in_port.data.lock().unwrap();
            let mut brw = data.borrow_mut();
            brw.extend(data_bytes);
        }

        loop {
            let avail = {
                let data = in_port.data.lock().unwrap();
                let brw = data.borrow();
                brw.len()
            };

            // check ReaderState
            {
                let mut lock = in_port.state.value.lock().unwrap();
                loop {
                    match lock.get() {
                        Some(ReaderState::Hungry(n)) => {
                            if avail >= n {
                                break;
                            } else {
                                return; // cannot fulfill via this write call
                            }
                        }
                        Some(ReaderState::Full) => {
                            break;
                        }
                        None => {}
                    }
                    lock = in_port.state.cond.wait(lock).unwrap();
                }
                lock.set(None);
            }

            {
                // signal that data is ready
                let lock = in_port.data_wait.value.lock().unwrap();
                lock.set(true);
                in_port.data_wait.cond.notify_one();
            }

            {
                // wait for the Data object to be dropped
                let mut lock = out_port.data_drop_signal.value.lock().unwrap();
                while !lock.get() {
                    lock = out_port.data_drop_signal.cond.wait(lock).unwrap();
                }
                lock.set(false);
            }
        }
    }
    fn read<'a, T: Copy>(&'a self, node: NodeID, port: InPortID, req: ReadRequest) -> Data<'a, T> {
        let in_node = self.graph.node(node);
        let in_port = in_node.in_port(port);
        match in_port.edge {
            Some(in_edge) => {
                //println!("connected: {:?} {:?}", node, port);
                let out_node = self.graph.node(in_edge.node);
                if out_node.attached {
                    let out_port = out_node.out_port(in_edge.port);

                    // TODO check how much data is available
                    let avail_bytes = {
                        let data = in_port.data.lock().unwrap();
                        let len = data.borrow().len(); // a weird quirk of borrowck
                        len
                    };

                    let avail_t = avail_bytes / mem::size_of::<T>();

                    use super::graph::ReaderState::*;
                    use super::graph::ReadRequest::*;
                    let state = match req {
                        Any => {
                            if avail_t == 0 {
                                Hungry(mem::size_of::<T>())
                            } else {
                                Full
                            }
                        }
                        Async => Full,
                        N(n) => {
                            if avail_t < n {
                                Hungry(n * mem::size_of::<T>())
                            } else {
                                Full
                            }
                        }
                        AsyncN(_) => Full,
                    };

                    // TODO if enough, continue

                    // TODO signal that we are waiting for data
                    {
                        let lock = in_port.state.value.lock().unwrap();
                        assert!(lock.get().is_none());
                        lock.set(Some(state));
                        in_port.state.cond.notify_one();
                    }

                    // TODO wait for data
                    {
                        let mut lock = in_port.data_wait.value.lock().unwrap();
                        while !lock.get() {
                            lock = in_port.data_wait.cond.wait(lock).unwrap();
                        }
                        lock.set(false); // no longer waiting
                    }

                    let out_data = {
                        let data_bytes = in_port.data.lock().unwrap();
                        let mut data_bytes = data_bytes.borrow_mut();
                        let avail_t = data_bytes.len() / mem::size_of::<T>();
                        let take_n = match req {
                            Any => avail_t,
                            Async => avail_t,
                            N(n) => {
                                assert!(avail_t >= n);
                                n
                            }
                            AsyncN(n) => {
                                if avail_t >= n {
                                    n
                                } else {
                                    0
                                }
                            }
                        };

                        let data: Vec<T> = unsafe {
                            slice::from_raw_parts(mem::transmute(data_bytes.as_ptr()), take_n)
                                .iter()
                                .cloned()
                                .collect()
                        };
                        data_bytes.drain(..(take_n * mem::size_of::<T>()));
                        data
                    };

                    /*{
                        let lock = in_port.state.value.lock().unwrap();
                        assert!(lock.get().is_some());
                        lock.set(None);
                        in_port.state.cond.notify_one();
                    }*/

                    Data::new(out_data, out_port.data_drop_signal.clone())
                } else {
                    panic!("endpoint not attached");
                }
            }
            None => {
                panic!("disconnected");
                // block until connected?
                // or error?
            }
        }
    }
}
