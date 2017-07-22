use std::sync::{Mutex, Condvar, Arc};
use std::cell::{Cell, RefCell};

// TODO Put this elsewhere
// clean it up
// get rid of copy constraint
// consider removing cell
#[derive(Debug, Default)]
pub struct CondvarCell<T: Copy> {
    pub value: Mutex<Cell<T>>,
    pub cond: Condvar,
}

impl<T: Copy> CondvarCell<T> {
    pub fn new(init: T) -> CondvarCell<T> {
        CondvarCell {
            value: Mutex::new(Cell::new(init)),
            cond: Condvar::new(),
        }
    }
}

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
    pub fn id(&self) -> NodeID {
        self.id
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

#[derive(Debug)]
pub enum ReadRequest {
    Async,
    AsyncN(usize),
    Any,
    N(usize),
}

/**
 * Optionally holds the edge that this port depends on.
 */
#[derive(Debug)]
pub struct InPort {
    pub edge: Option<OutEdge>,
    pub data_wait: CondvarCell<bool>,
    pub data: Mutex<RefCell<Vec<u8>>>,
}

impl Clone for InPort {
    fn clone(&self) -> Self {
        InPort::new(self.edge)
    }
}

impl InPort {
    fn new(edge: Option<OutEdge>) -> InPort {
        InPort {
            edge: edge,
            data_wait: CondvarCell::new(false),
            data: Mutex::new(RefCell::new(Vec::new())),
        }
    }
    /**
     * Construct a void port. Void ports are not connected.
     *
     * todo kill me
     */
    fn void() -> InPort {
        InPort::new(None)
    }
}

/**
 * Has a vector of Edges that depend on this port.
 */
#[derive(Debug)]
pub struct OutPort {
    pub edge: Option<InEdge>,
    pub data_drop_signal: Arc<CondvarCell<bool>>,
}

impl OutPort {
    fn new(edge: Option<InEdge>) -> OutPort {
        OutPort {
            edge: edge,
            data_drop_signal: Arc::new(CondvarCell::new(false)),
        }
    }
    /**
     * Construct a void port. Void ports are not connected.
     * todo kill me
     */
    fn void() -> OutPort {
        OutPort::new(None)
    }
}

impl Clone for OutPort {
    fn clone(&self) -> Self {
        OutPort::new(self.edge)
    }
}
