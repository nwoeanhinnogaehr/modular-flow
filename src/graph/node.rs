#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NodeID(pub usize);
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InPortID(pub usize);
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutPortID(pub usize);

/**
 * A node is a processing unit.
 *
 * It contains an ID, used as a global reference, a vector of input ports, and a vector of output
 * ports.
 *
 * At the moment there is no way for user code to get a mut Node.
 */
pub struct Node {
    id: NodeID,
    in_ports: Vec<InPort>,
    out_ports: Vec<OutPort>,
    pub attached: bool,
    variadic: bool,
}

impl Node {
    /**
     * Construct a new `Node` with the given ID, number of input ports, and number of output ports.
     *
     * User code should never need to call this. So for now, it is public to the crate only.
     * User code should use `Graph::add_node` instead.
     */
    pub(crate) fn new(id: NodeID, num_in: usize, num_out: usize) -> Node {
        Node {
            id,
            in_ports: vec![InPort::void(); num_in],
            out_ports: vec![OutPort::void(); num_out],
            attached: false,
            variadic: false,
        }
    }
    pub(crate) fn new_variadic(id: NodeID) -> Node {
        Node {
            id,
            in_ports: Vec::new(),
            out_ports: Vec::new(),
            attached: false,
            variadic: true,
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
            NodeType::Observer
        } else if self.has_outputs() {
            NodeType::Source
        } else
        /* if self.has_inputs() */
        {
            NodeType::Sink
        }
    }
    pub fn in_port(&self, port_id: InPortID) -> &InPort {
        &self.in_ports[port_id.0]
    }
    pub fn out_port(&self, port_id: OutPortID) -> &OutPort {
        &self.out_ports[port_id.0]
    }
    pub fn in_port_mut(&mut self, port_id: InPortID) -> &mut InPort {
        &mut self.in_ports[port_id.0]
    }
    pub fn out_port_mut(&mut self, port_id: OutPortID) -> &mut OutPort {
        &mut self.out_ports[port_id.0]
    }
    pub fn push_in_port(&mut self) -> Result<usize, ()> {
        if self.variadic {
            self.in_ports.push(InPort::void());
            Ok(self.in_ports.len() - 1)
        } else {
            Err(())
        }
    }
    pub fn push_out_port(&mut self) -> Result<usize, ()> {
        if self.variadic {
            self.out_ports.push(OutPort::void());
            Ok(self.out_ports.len() - 1)
        } else {
            Err(())
        }
    }
    pub fn pop_in_port(&mut self) -> Result<usize, ()> {
        if self.variadic {
            self.in_ports
                .pop()
                .map_or(Err(()), |_| Ok(self.in_ports.len()))
        } else {
            Err(())
        }
    }
    pub fn pop_out_port(&mut self) -> Result<usize, ()> {
        if self.variadic {
            self.out_ports
                .pop()
                .map_or(Err(()), |_| Ok(self.in_ports.len()))
        } else {
            Err(())
        }
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
    /// A `Observer` Node has no input or output ports.
    Observer,
}

/**
 * An edge is like a vector pointing to a specific port of a specific node, originating from
 * nowhere in particular.
 */
#[derive(Copy, Clone, Debug)]
pub struct InEdge {
    pub node: NodeID,
    pub port: InPortID,
}
impl InEdge {
    pub fn new(node: NodeID, port: InPortID) -> InEdge {
        InEdge { node, port }
    }
}
#[derive(Copy, Clone, Debug)]
pub struct OutEdge {
    pub node: NodeID,
    pub port: OutPortID,
}
impl OutEdge {
    pub fn new(node: NodeID, port: OutPortID) -> OutEdge {
        OutEdge { node, port }
    }
}

/**
 * Optionally holds the edge that this port depends on.
 */
#[derive(Clone, Debug)]
pub struct InPort {
    pub edge: Option<OutEdge>,
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
#[derive(Clone, Debug)]
pub struct OutPort {
    pub edge: Option<InEdge>,
}

impl OutPort {
    /**
     * Construct a void port. Void ports are not connected.
     */
    fn void() -> OutPort {
        OutPort { edge: None }
    }
}
