/**
 * A node is a processing unit.
 *
 * It contains an ID, used as a global reference, a vector of input ports, and a vector of output
 * ports.
 *
 * At the moment there is no way for user code to get a mut Node.
 */
pub struct Node {
    id: usize,
    in_ports: Vec<InPort>,
    out_ports: Vec<OutPort>
}

impl Node {
    /**
     * Construct a new `Node` with the given ID, number of input ports, and number of output ports.
     *
     * User code should never need to call this. So for now, it is public to the crate only.
     * User code should use `Graph::add_node` instead.
     */
    pub(crate) fn new(id: usize, num_in: usize, num_out: usize) -> Node {
        Node {
            id,
            in_ports: vec![InPort::void(); num_in],
            out_ports: vec![OutPort::void(); num_out],
        }
    }
    pub fn has_inputs(&self) -> bool {
        !self.in_ports.is_empty()
    }
    pub fn has_outputs(&self) -> bool {
        !self.out_ports.is_empty()
    }
    pub fn node_type(&self) -> NodeType {
        if self.has_inputs() && self.has_outputs() {
            NodeType::Internal
        } else if !self.has_inputs() && !self.has_outputs() {
            NodeType::Useless
        } else if self.has_outputs() {
            NodeType::Source
        } else /* if self.has_inputs() */ {
            NodeType::Sink
        }
    }
    pub fn in_port(&self, port_id: usize) -> &InPort {
        &self.in_ports[port_id]
    }
    pub fn out_port(&self, port_id: usize) -> &OutPort {
        &self.out_ports[port_id]
    }
    pub fn in_port_mut(&mut self, port_id: usize) -> &mut InPort {
        &mut self.in_ports[port_id]
    }
    pub fn out_port_mut(&mut self, port_id: usize) -> &mut OutPort {
        &mut self.out_ports[port_id]
    }
}

/**
 * The type of a node is derived from the structure of its ports.
 */
#[derive(PartialEq, Eq, Debug)]
pub enum NodeType {
    /// A `Source` Node has no input ports.
    Source,
    /// An `Internal` Node has both input and output ports.
    Internal,
    /// A `Sink` Node has no output ports.
    Sink,
    /// A `Useless` Node has no input or output ports.
    Useless
}

/**
 * An edge is like a vector pointing to a specific port of a specific node, originating from
 * nowhere in particular.
 */
#[derive(Copy, Clone)]
pub struct Edge {
    pub node_id: usize,
    pub port_id: usize
}

/**
 * Optionally holds the edge that this port depends on.
 */
#[derive(Clone)]
pub struct InPort {
    pub edge: Option<Edge>
}

impl InPort {
    /**
     * Construct a void port. Void ports are not connected.
     */
    fn void() -> InPort {
        InPort { edge: None }
    }
}

/**
 * Has a vector of Edges that depend on this port.
 */
#[derive(Clone)]
pub struct OutPort {
    pub edges: Vec<Edge>
}

impl OutPort {
    /**
     * Construct a void port. Void ports are not connected.
     */
    fn void() -> OutPort {
        OutPort { edges: Vec::new() }
    }
}
