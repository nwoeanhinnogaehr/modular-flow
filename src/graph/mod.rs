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
    pub fn get_node(&self, node_id: usize) -> &Node {
        &self.nodes[node_id]
    }
    pub fn add_node(&mut self, num_in: usize, num_out: usize) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node::new(id, num_in, num_out));
        id
    }
    /**
     * Connects output port `src_port` of node `src_id` to input port `dst_port` of node `dst_id`.
     *
     * Returns Err if `dst_port` is already connected. You must call `disconnect` on the destination first.
     */
    pub fn connect(&mut self, src_id: usize, src_port: usize, dst_id: usize, dst_port: usize) -> Result<(), ()> {
        if self.nodes[dst_id].in_port(dst_port).edge.is_some() {
            Err(())
        } else {
            self.nodes[src_id].out_port_mut(src_port).edges.push(Edge { node_id: dst_id, port_id: dst_port });
            self.nodes[dst_id].in_port_mut(dst_port).edge = Some(Edge { node_id: src_id, port_id: src_port });
            Ok(())
        }
    }
    /**
     * Disconnect an input port from the output port it is connected to.
     *
     * Returns Err if the port is not connected.
     */
    pub fn disconnect(&mut self, node_id: usize, port_id: usize) -> Result<(), ()> {
        if let Some(edge) = self.nodes[node_id].in_port(port_id).edge {
            self.nodes[edge.node_id].out_port_mut(edge.port_id).edges
                .retain(|&x| !(x.node_id == node_id && x.port_id == port_id));
            self.nodes[node_id].in_port_mut(port_id).edge = None;
            Ok(())
        } else {
            Err(())
        }
    }
}
