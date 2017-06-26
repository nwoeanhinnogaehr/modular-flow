use super::graph::*;
use std::sync::Arc;
use std::mem;
use std::ptr;
use std::slice;
use std::cmp::{min, max};

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext {
    id: NodeID,
    sched: Arc<Scheduler>
}

pub type Data<T> = [T];

impl NodeContext {
    /// Async, may return empty data
    pub fn read_async<T>(&self, port: InPortID) -> &Data<T> {
        self.read(port, ReadRequest::Async).wait()
    }
    /// Returned data is either empty or of size `n`
    pub fn read_n_async<T>(&self, port: InPortID, n: usize) -> &Data<T> {
        self.read(port, ReadRequest::AsyncN(n)).wait()
    }
    /// Block until at least one byte is available
    pub fn read_any<T>(&self, port: InPortID) -> &Data<T> {
        self.read(port, ReadRequest::Any).wait()
    }
    pub fn read_n<T>(&self, port: InPortID, n: usize) -> &Data<T> {
        self.read(port, ReadRequest::N(n)).wait()
    }
    pub fn read_into<T>(&self, port: InPortID, dest: &mut Data<T>) {
        self.read(port, ReadRequest::Into(dest)).wait();
    }
    fn read<'a, T>(&'a self, port: InPortID, req: ReadRequest<'a, T>) -> FutureRead<'a, T> {
        self.sched.queue_read(self.id, port, req)
    }
    pub fn write<T>(&self, port: OutPortID, src: &Data<T>) {
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
    Into(&'a mut Data<T>)
}
struct FutureRead<'a, T: 'a> {
    data: &'a Data<T>
}
impl<'a, T> FutureRead<'a, T> {
    fn wait(&self) -> &'a Data<T> {
        // TODO wait until signal that data is complete
        &self.data
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
    fn write<'a, T>(&self, node: NodeID, port: OutPortID, data: &'a Data<T>) {
        let data_bytes = unsafe {
            slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() * mem::size_of::<T>())
        };
        let node_ref = self.graph.node(node);
        let port = node_ref.out_port(port);
        for edge in &port.edges {
            let mut dest_node = self.graph.node_mut(edge.node_id);
            let mut dest_port = dest_node.in_port_mut(edge.port_id);
            dest_port.buffer.extend(data_bytes);
        }
    }
    fn queue_read<'a, T>(&'a self, node: NodeID, port: InPortID, req: ReadRequest<'a, T>) -> FutureRead<'a, T> {
        let node_ref = self.graph.node(node);
        let port = node_ref.in_port(port);
        match port.edge {
            Some(edge) => {
                println!("connected: {:?} {:?}", node, port);
                if self.graph.node(edge.node_id).attached {
                    //println!("endpoint attached");
                    use self::ReadRequest::*;
                    // TODO oops better watch out for ZSTs
                    let max_avail = port.buffer.len() / mem::size_of::<T>();
                    let len = match req {
                        N(v) | AsyncN(v) => min(v, max_avail),
                        Into(v) => min(v.len(), max_avail),
                        Async | Any => max_avail
                    };
                    FutureRead {
                        data: unsafe { slice::from_raw_parts(mem::transmute(port.buffer.as_ptr()), len) }
                    }
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
