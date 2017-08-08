use super::graph::*;
use std::sync::{Arc, MutexGuard};

/**
 * A `NodeGuard` holds a lock on the `Graph`, which prevents other threads from interacting with
 * the graph while this object is in scope (except if execution is in a call to `wait`; see below
 * for more details).
 */
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

    /**
     * Block until the given function returns true. This function unlocks the graph while it waits
     * for events. The given function will be polled whenever a read or write operation occurs in
     * another node. It is given an immutable reference to this `NodeGuard` as a parameter, which
     * can be used to check availability of data.
     */
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

    /**
     * Write `data` to `port`.
     */
    pub fn write(&mut self, port: OutPortID, data: &[u8]) {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let mut buffer = in_port.data.lock().unwrap();
        buffer.extend(data.iter());
        self.graph.cond.notify_all();
    }

    /**
     * Read all available data from `port`.
     */
    pub fn read(&mut self, port: InPortID) -> Vec<u8> {
        let in_port = self.node.in_port(port);
        let mut buffer = in_port.data.lock().unwrap();
        let out = buffer.drain(..).collect();
        self.graph.cond.notify_all();
        out
    }

    /**
     * Read exactly `n` bytes of data from `port`. If `n` bytes are not available, `None` is
     * returned.
     */
    pub fn read_n(&mut self, port: InPortID, n: usize) -> Option<Vec<u8>> {
        let in_port = self.node.in_port(port);
        let mut buffer = in_port.data.lock().unwrap();
        if buffer.len() < n {
            return None;
        }
        let out = buffer.drain(..n).collect();
        self.graph.cond.notify_all();
        Some(out)
    }

    /**
     * Returns the number of bytes available to be read from the given input port.
     */
    pub fn available(&self, port: InPortID) -> usize {
        let in_port = self.node.in_port(port);
        let buffer = in_port.data.lock().unwrap();
        buffer.len()
    }

    /**
     * Returns the number of bytes buffered (i.e. waiting to be read) from the given output port.
     */
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
    /**
     * Lock the graph, returning a `NodeGuard` which can be used for interacting with other nodes.
     */
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
 * A `Context` takes a `Graph` and enables connecting threads to nodes.
 */
pub struct Context {
    graph: Arc<Graph>,
}

impl Context {
    /**
     * Construct a new `Context` using the given `Graph`.
     */
    pub fn new(graph: Graph) -> Context {
        Context {
            graph: Arc::new(graph),
        }
    }

    /**
     * Create the context for a specific node. This context is required for the node to interact
     * with others. The returned object can be safely moved into a thread.
     *
     * Returns `Err` if this node is already attached to another thread.
     */
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext, ()> {
        self.graph.node(node).attach_thread().map(|_| {
            NodeContext {
                graph: self.graph.clone(),
                id: node,
            }
        })
    }
}
