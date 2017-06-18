/// node
pub mod node;

pub use self::node::*;

pub struct Graph {
    nodes: Vec<Node>,
}

impl Graph {
    pub fn new() -> Graph {
        Graph {
            nodes: Vec::new()
        }
    }
    /**
     * Get a node by ID.
     */
    pub fn node(&self, node_id: NodeID) -> &Node {
        &self.nodes[node_id.0]
    }
    fn node_mut(&mut self, node_id: NodeID) -> &mut Node {
        &mut self.nodes[node_id.0]
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
     * Add a node with a variable number of input and output ports.
     * The ID of the newly created node is returned.
     */
    pub fn add_variadic_node(&mut self) -> NodeID {
        let id = NodeID(self.nodes.len());
        self.nodes.push(Node::new_variadic(id));
        id
    }
    /**
     * Connects output port `src_port` of node `src_id` to input port `dst_port` of node `dst_id`.
     *
     * Returns Err if `dst_port` is already connected. You must call `disconnect` on the destination first.
     */
    pub fn connect(&mut self, src_id: NodeID, src_port: OutPortID, dst_id: NodeID, dst_port: InPortID) -> Result<(), ()> {
        if self.node(dst_id).in_port(dst_port).edge.is_some() {
            Err(())
        } else {
            self.node_mut(src_id).out_port_mut(src_port).edges.push(InEdge { node_id: dst_id, port_id: dst_port });
            self.node_mut(dst_id).in_port_mut(dst_port).edge = Some(OutEdge { node_id: src_id, port_id: src_port });
            Ok(())
        }
    }
    /**
     * Disconnect an input port from the output port it is connected to, returning the edge that
     * was removed.
     *
     * Returns Err if the port is not connected.
     */
    pub fn disconnect(&mut self, node_id: NodeID, port_id: InPortID) -> Result<OutEdge, ()> {
        if let Some(edge) = self.node(node_id).in_port(port_id).edge {
            self.node_mut(edge.node_id).out_port_mut(edge.port_id).edges
                .retain(|&x| !(x.node_id == node_id && x.port_id == port_id));
            self.node_mut(node_id).in_port_mut(port_id).edge.take().ok_or(())
        } else {
            Err(())
        }
    }
}
