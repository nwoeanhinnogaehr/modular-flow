use super::graph::*;
use std::marker::PhantomData;

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext<'a> {
    graph: &'a Graph,
    id: NodeID,
    supervisor: &'a Supervisor
}

// TODO these should not be implemented over slices, in case we want to do some weird rope shit
// later
impl<'a> NodeContext<'a> {
    pub fn read_any<T>(&self) -> &[T] {
        self.supervisor.queue_read::<T>(ReadRequest::Any).wait()
    }
    pub fn read_n<T>(&mut self, n: usize) -> &[T] {
        self.supervisor.queue_read::<T>(ReadRequest::N(n)).wait()
    }
    pub fn read_into<T>(&mut self, dest: &mut [T]) {
        self.supervisor.queue_read::<T>(ReadRequest::Into(dest)).wait();
    }
    pub fn write<T>(&mut self, src: &[T]) {
        unimplemented!();
    }
}

enum ReadRequest<'a, T: 'a> {
    Any,
    N(usize),
    Into(&'a mut [T])
}
struct FutureRead<'a, T: 'a> {
    data: &'a [T]
}
impl<'a, T> FutureRead<'a, T> {
    fn wait(&self) -> &'a [T] {
        // TODO wait until signal that data is complete
        &self.data
    }
}

/**
 * The supervisor runs the main loop. It owns the graph, and manages access to the graph while the
 * application is running. Node threads must be attached to the supervisor by requesting a context.
 */
pub struct Supervisor {
    graph: Graph,
}

impl Supervisor {
    pub fn new(graph: Graph) -> Supervisor {
        Supervisor {
            graph
        }
    }
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext<'a>, ()> {
        // TODO prevent multiple contexts being lent for the same node
        Ok(NodeContext {
            graph: &self.graph,
            id: node,
            supervisor: &self
        })
    }
    pub fn run(&mut self) {
        loop {

        }
    }

    fn queue_read<'a, T>(&'a self, req: ReadRequest<'a, T>) -> FutureRead<'a, T> {
        unimplemented!();
    }
}
