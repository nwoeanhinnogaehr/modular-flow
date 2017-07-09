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
    sched: Arc<Scheduler>
}

pub struct Data<'a, T: 'a> {
    data: Vec<T>,
    phantom: PhantomData<&'a T>
}

impl<'a, T: 'a> Data<'a, T> {
    pub fn new(data: Vec<T>) -> Data<'a, T> {
        Data {
            data,
            phantom: Default::default()
        }
    }
}

pub struct MutData<'a, T: 'a> {
    data: Vec<T>,
    phantom: PhantomData<&'a T>
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
    fn read<'a, T: Copy>(&'a self, port: InPortID, req: ReadRequest<'a, T>) -> FutureRead<'a, T> {
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

enum ReadRequest<'a, T: 'a> {
    Async,
    AsyncN(usize),
    Any,
    N(usize),
    Into(MutData<'a, T>)
}
struct FutureRead<'a, T: 'a> {
    data: Data<'a, T>
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
        // TODO prevent multiple contexts being lent for the same node
        self.sched.graph.attach_thread(node).map(|_|
            NodeContext {
                sched: self.sched.clone(),
                id: node,
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
        Scheduler {
            graph,
        }
    }
    fn write<'a, T>(&self, node: NodeID, port: OutPortID, data: Data<T>) {
        /*let data_bytes = unsafe {
            slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() * mem::size_of::<T>())
        };*/
        /*
        let mut out_node = self.graph.node_mut(node);
        let mut out_port = out_node.out_port_mut(port);

        for in_edge in &out_port.edges {
            let in_node = self.graph.node(in_edge.node);
            let in_port = in_node.in_port(in_edge.port);
            // hmm what if the port isn't blocked?
            // need to store data somewhere where the port can get it when it needs it.
        }
        */
    }
    fn queue_read<'a, T: Copy>(&'a self, node: NodeID, port: InPortID, req: ReadRequest<'a, T>) -> FutureRead<'a, T> {
        /*let mut in_node = self.graph.node_mut(node);
        let mut in_port = in_node.in_port_mut(port);
        match in_port.edge {
            Some(in_edge) => {
                println!("connected: {:?} {:?}", node, port);
                let mut out_node = self.graph.node_mut(in_edge.node);
                if out_node.attached {
                    let mut out_port = out_node.out_port_mut(in_edge.port);
                    //println!("endpoint attached");
                    use self::ReadRequest::*;
                    // TODO oops better watch out for ZSTs
                    let max_avail = out_port.buffer.len() / mem::size_of::<T>();
                    let len = match req {
                        N(v) | AsyncN(v) => min(v, max_avail),
                        Into(v) => min(v.len(), max_avail),
                        Async | Any => max_avail
                    };
                    let rd = FutureRead {
                        data: Data {
                            data: unsafe {
                                slice::from_raw_parts(mem::transmute(out_port.buffer.as_ptr()), len)
                            }.iter().cloned().collect(),
                            phantom: Default::default()
                        }
                    };
                    out_port.buffer.clear();
                    rd
                } else {
                    panic!("endpoint not attached");
                }
            },
            None => {
                panic!("disconnected");
                // block until connected?
                // or error?
            }
        }*/
        unimplemented!();
    }

}
