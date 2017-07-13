use super::graph::*;
use std::sync::Arc;
use std::mem;
use std::ptr;
use std::slice;
use std::cmp::{min, max};
use std::marker::PhantomData;
use std::ops::Deref;

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext {
    id: NodeID,
    sched: Arc<Scheduler>,
}

pub struct Data<'a, T: 'a> {
    data: Vec<T>,
    phantom: PhantomData<&'a T>,
}

impl<'a, T: 'a> Data<'a, T> {
    pub fn new(data: Vec<T>) -> Data<'a, T> {
        Data {
            data,
            phantom: Default::default(),
        }
    }
}

pub struct MutData<'a, T: 'a> {
    data: Vec<T>,
    phantom: PhantomData<&'a T>,
}

impl<'a, T> Deref for Data<'a, T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<'a, T> Deref for MutData<'a, T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl NodeContext {
    /// Async, may return empty data
    pub fn read_async<T: Copy>(&self, port: InPortID) -> Data<T> {
        self.read(port, ReadRequest::Async).wait()
    }
    /// Returned data is either empty or of size `n`
    pub fn read_n_async<T: Copy>(&self, port: InPortID, n: usize) -> Data<T> {
        self.read(port, ReadRequest::AsyncN(n)).wait()
    }
    /// Block until at least one byte is available
    pub fn read_any<T: Copy>(&self, port: InPortID) -> Data<T> {
        self.read(port, ReadRequest::Any).wait()
    }
    pub fn read_n<T: Copy>(&self, port: InPortID, n: usize) -> Data<T> {
        self.read(port, ReadRequest::N(n)).wait()
    }
    /*pub fn read_into<T: Clone>(&self, port: InPortID, dest: MutData<T>) {
        self.read(port, ReadRequest::Into(dest)).wait();
    }*/
    fn read<'a, T: Copy>(&'a self, port: InPortID, req: ReadRequest) -> FutureRead<'a, T> {
        self.sched.queue_read(self.id, port, req)
    }
    pub fn write<T: Copy>(&self, port: OutPortID, src: Data<T>) {
        self.sched.write(self.id, port, src);
    }
}

impl Drop for NodeContext {
    fn drop(&mut self) {
        self.sched.graph.detach_thread(self.id).unwrap();
    }
}

struct FutureRead<'a, T: 'a> {
    data: Data<'a, T>,
}
impl<'a, T> FutureRead<'a, T> {
    fn wait(self) -> Data<'a, T> {
        // TODO wait until signal that data is complete
        self.data
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
        loop {}
    }
}

pub struct Scheduler {
    graph: Graph,
}

impl Scheduler {
    fn new(graph: Graph) -> Scheduler {
        Scheduler { graph }
    }
    fn write<'a, T>(&self, node: NodeID, port: OutPortID, data: Data<T>) {
        let data_bytes: &[u8] = unsafe {
            slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() * mem::size_of::<T>())
        };

        let out_node = self.graph.node(node);
        let out_port = out_node.out_port(port);

        let in_edge = out_port.edge.unwrap();
        let in_node = self.graph.node(in_edge.node);
        let in_port = in_node.in_port(in_edge.port);

        //TODO
        // determine how much data the in port is waiting for (if any)
        // i.e. check current readrequest

        // "setup pointers"
        // or if it isn't waiting, copy data so it can go ahead on it's own
        {
            let data = in_port.data.lock().unwrap();
            data.borrow_mut().extend(data_bytes);
        }

        // TODO
        // signal that data is ready
        // with a condition variable?
        // with a channel?

        // TODO wait for pointers to be grabbed: no longer waiting

        { // wait for next read to begin
            let mut lock = in_port.read_begin.mx.lock().unwrap();
            while !lock.get() {
                lock = in_port.read_begin.cv.wait(lock).unwrap();
            }
            lock.set(false);
        }

        // TODO destroy data

        // but make sure not to let multiple writes happen before a read (maybe)
        // there's a problem here: a writer going much faster than the corresponsing reader
        // may fill up the memory. but if we block, it may lead to deadlock or non-rt processing.
    }
    fn queue_read<'a, T: Copy>(
        &'a self,
        node: NodeID,
        port: InPortID,
        req: ReadRequest,
    ) -> FutureRead<'a, T> {
        let in_node = self.graph.node(node);
        let in_port = in_node.in_port(port);
        match in_port.edge {
            Some(in_edge) => {
                //println!("connected: {:?} {:?}", node, port);
                let out_node = self.graph.node(in_edge.node);
                if out_node.attached {
                    let out_port = out_node.out_port(in_edge.port);

                    { // signal begin of read
                        let mut lock = in_port.read_begin.mx.lock().unwrap();
                        lock.set(true);
                        in_port.read_begin.cv.notify_one();
                    }

                    // TODO check how much data is available
                    let avail_bytes = {
                        let data = in_port.data.lock().unwrap();
                        let len = data.borrow().len();
                        len
                    };

                    // TODO if enough, continue

                    // TODO signal that we are waiting for data

                    // TODO wait for data

                    let out_data = {
                        let data_bytes = in_port.data.lock().unwrap();
                        let mut data_bytes = data_bytes.borrow_mut();
                        let data: Vec<T> = unsafe {
                            slice::from_raw_parts(mem::transmute(data_bytes.as_ptr()), data_bytes.len() / mem::size_of::<T>()).iter().cloned().collect()
                        };
                        data_bytes.clear();
                        data
                    };

                    // TODO signal that we are no longer waiting for data

                    FutureRead {
                        data: Data {
                            data: out_data,
                            phantom: PhantomData
                        }
                    }
                    // check how much data is available
                    // if enough, take it
                    // else, signal how much we want then sleep
                    // once we wake up, take it
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
