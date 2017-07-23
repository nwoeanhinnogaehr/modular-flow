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

/// TODO RENAME ME
fn max_avail<T>(data: &[u8]) -> usize {
    data.len() / mem::size_of::<T>()
}

impl Scheduler {
    fn new(graph: Graph) -> Scheduler {
        Scheduler { graph }
    }
    fn write<'a, T>(&self, node: NodeID, port: OutPortID, data: &[T]) {
        let data_bytes: &[u8] = unsafe {
            slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() * mem::size_of::<T>())
        };

        let out_node = self.graph.node(node);
        let out_port = out_node.out_port(port);

        let in_edge = out_port.edge.unwrap();
        let in_node = self.graph.node(in_edge.node);
        let in_port: &InPort = in_node.in_port(in_edge.port);



        // "setup pointers"
        // or if it isn't waiting, copy data so it can go ahead on it's own
        let avail_t = {
            let data = in_port.data.lock().unwrap();
            data.borrow_mut().extend(data_bytes);
            let n = max_avail::<T>(&*data.borrow());
            n
        };

        //TODO
        // determine how much data the in port is waiting for (if any)
        // i.e. check current readrequest
        {
            let mut req_lock = in_port.req.value.lock().unwrap();
            loop {
                match req_lock.get() {
                    Some(req) => {
                        use super::graph::ReadRequest::*;
                        match req {
                            Any | Async => { },
                            N(n) | AsyncN(n) => {
                                if n > avail_t {
                                    return;
                                }
                            },
                        }
                        break;
                    },
                    None => {
                    },
                }
                req_lock = in_port.req.cond.wait(req_lock).unwrap();
            }
        }

        { // signal that data is ready
            let lock = in_port.data_wait.value.lock().unwrap();
            lock.set(true);
            in_port.data_wait.cond.notify_one();
        }

        // TODO wait for pointers to be grabbed: no longer waiting
        // is this necessary???

        { // wait for the Data object to be dropped
            let mut lock = out_port.data_drop_signal.value.lock().unwrap();
            while !lock.get() {
                lock = out_port.data_drop_signal.cond.wait(lock).unwrap();
            }
            lock.set(false);
        }

        // TODO safe to destroy data here
        {
            // destroy data
            //let data = in_port.data.lock().unwrap();
            //data.borrow_mut().clear();

        }
    }
    fn read<'a, T: Copy>(
        &'a self,
        node: NodeID,
        port: InPortID,
        req: ReadRequest,
    ) -> Data<'a, T> {
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

                    use super::graph::ReadRequest::*;
                    let need_data = match req {
                        Any => avail_t == 0,
                        Async => false,
                        N(n) => avail_t < n,
                        AsyncN(_) => false,
                    };

                    // TODO if enough, continue

                    // TODO signal that we are waiting for data
                    {
                        let port_req = in_port.req.value.lock().unwrap();
                        assert!(port_req.get().is_none());
                        port_req.set(Some(req));
                        in_port.req.cond.notify_one();
                    }

                    // TODO wait for data
                    {
                        let mut lock = in_port.data_wait.value.lock().unwrap();
                        if need_data {
                            while !lock.get() {
                                lock = in_port.data_wait.cond.wait(lock).unwrap();
                            }
                        }
                        lock.set(false); // no longer waiting
                    }

                    let out_data = {
                        let data_bytes = in_port.data.lock().unwrap();
                        let mut data_bytes = data_bytes.borrow_mut();
                        let avail_t = max_avail::<T>(&*data_bytes);
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
                            slice::from_raw_parts(mem::transmute(data_bytes.as_ptr()), take_n).iter().cloned().collect()
                        };
                        data_bytes.drain(..(take_n * mem::size_of::<T>()));
                        data
                    };

                    // TODO signal that we are no longer waiting for data

                    {
                        let port_req = in_port.req.value.lock().unwrap();
                        assert!(port_req.get().is_some());
                        port_req.set(None);
                        in_port.req.cond.notify_one();
                    }

                    Data::new(out_data, out_port.data_drop_signal.clone())
                } else {
                    panic!("endpoint not attached");
                }
            },
            None => {
                panic!("disconnected");
                // block until connected?
                // or error?
            }
        }
    }
}
