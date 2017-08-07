use super::graph::*;
use std::sync::{Arc, MutexGuard};
use std::ops::Deref;
use std::thread;

pub struct Data<T> {
    data: Vec<T>,
}

impl<T> Data<T> {
    pub fn new(data: Vec<T>) -> Data<T> {
        Data { data }
    }
}

impl<T> Deref for Data<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub struct NodeGuard<'a> {
    node: &'a Node,
    graph: &'a Graph,
    guard: Option<MutexGuard<'a, ()>>,
}

impl<'a> NodeGuard<'a> {
    fn new(graph: &'a Graph, node_id: NodeID) -> NodeGuard<'a> {
        let node = graph.node(node_id);
        let guard = Some(graph.lock.lock().unwrap());
        NodeGuard { node, graph, guard }
    }
    pub fn wait<F>(&mut self, mut cond: F)
    where
        F: FnMut(&Self) -> bool,
    {
        let mut guard = self.guard.take().unwrap();
        while !cond(self) {
            guard = self.graph.cond.wait(guard).unwrap();
        }
        self.guard = Some(guard);
    }
    pub fn write(&mut self, port: OutPortID, data: &[u8]) {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let mut buffer = in_port.data.lock().unwrap();
        buffer.extend(data.iter());
        self.graph.cond.notify_all();
    }
    pub fn read(&mut self, port: InPortID) -> Data<u8> {
        let in_port = self.node.in_port(port);
        let mut buffer = in_port.data.lock().unwrap();
        let out = buffer.drain(..).collect();
        self.graph.cond.notify_all();
        Data::new(out)
    }
    pub fn read_n(&mut self, port: InPortID, n: usize) -> Data<u8> {
        let in_port = self.node.in_port(port);
        let mut buffer = in_port.data.lock().unwrap();
        if buffer.len() < n {
            return Data::new(Vec::new());
        }
        let out = buffer.drain(..n).collect();
        self.graph.cond.notify_all();
        Data::new(out)
    }
    pub fn available(&self, port: InPortID) -> usize {
        let in_port = self.node.in_port(port);
        let buffer = in_port.data.lock().unwrap();
        buffer.len()
    }
    pub fn buffered(&self, port: OutPortID) -> usize {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let buffer = in_port.data.lock().unwrap();
        buffer.len()
    }
    // read_while, ...
}

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext {
    id: NodeID,
    graph: Arc<Graph>,
}

impl NodeContext {
    pub fn lock<'a>(&'a self) -> NodeGuard<'a> {
        NodeGuard::new(&*self.graph, self.id)
    }
}

impl Drop for NodeContext {
    fn drop(&mut self) {
        self.graph.node(self.id).detach_thread().unwrap();
    }
}

/**
 * The supervisor runs the main loop. It owns the graph, and manages access to the graph while the
 * application is running. Node threads must be attached to the supervisor by requesting a context.
 */
pub struct Context {
    graph: Arc<Graph>,
}

impl Context {
    pub fn new(graph: Graph) -> Context {
        Context {
            graph: Arc::new(graph),
        }
    }
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext, ()> {
        self.graph.node(node).attach_thread().map(|_| {
            NodeContext {
                graph: self.graph.clone(),
                id: node,
            }
        })
    }
    pub fn run(self) {
        thread::park();
    }
}
