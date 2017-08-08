use std::sync::{Condvar, Mutex};

/// node
pub mod node;

pub use self::node::*;

/**
 * A graph contains many `Node`s and connections between their `Port`s.
 */
pub struct Graph {
    nodes: Vec<Node>,
    pub(crate) cond: Condvar,
    pub(crate) lock: Mutex<()>,
}

impl Graph {
    /**
     * Create a new empty graph.
     */
    pub fn new() -> Graph {
        Graph {
            nodes: Vec::new(),
            cond: Condvar::new(),
            lock: Mutex::new(()),
        }
    }

    /**
     * Get a node by ID.
     */
    pub fn node(&self, node_id: NodeID) -> &Node {
        &self.nodes[node_id.0]
    }

    /**
     * Add a node with a fixed number of input and output ports.
     * The ID of the newly created node is returned.
     */
    pub fn add_node(&mut self, num_in: usize, num_out: usize) -> NodeID {
        let id = NodeID(self.nodes.len());
        self.nodes.push(Node::new(id, num_in, num_out));
        id
    }

    /**
     * Connects output port `src_port` of node `src_id` to input port `dst_port` of node `dst_id`.
     *
     * Returns Err if `src_port` or `dst_port` is already connected.
     * You must call `disconnect` on the destination first.
     */
    pub fn connect(
        &self,
        src_id: NodeID,
        src_port: OutPortID,
        dst_id: NodeID,
        dst_port: InPortID,
    ) -> Result<(), ()> {
        let in_port = self.node(dst_id).in_port(dst_port);
        let out_port = self.node(src_id).out_port(src_port);
        if in_port.has_edge() || out_port.has_edge() {
            Err(())
        } else {
            in_port.set_edge(Some(OutEdge::new(src_id, src_port)));
            out_port.set_edge(Some(InEdge::new(dst_id, dst_port)));
            Ok(())
        }
    }

    /**
     * Disconnect an input port from the output port it is connected to, returning the edge that
     * was removed.
     *
     * Returns Err if the port is not connected.
     */
    pub fn disconnect(&self, node_id: NodeID, port_id: InPortID) -> Result<OutEdge, ()> {
        let in_port = self.node(node_id).in_port(port_id);
        if let Some(edge) = in_port.edge() {
            in_port.set_edge(None);
            self.node(edge.node).out_port(edge.port).set_edge(None);
            Ok(edge)
        } else {
            Err(())
        }
    }
}
