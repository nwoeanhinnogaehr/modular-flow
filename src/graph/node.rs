use std::sync::{Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

/// A unique identifier of any `Node` in a specific `Graph`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NodeID(pub usize);

/// A unique identifier of any `InPort` in a specific `Node`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InPortID(pub usize);

/// A unique identifier of any `OutPort` in a specific `Node`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutPortID(pub usize);

/**
 * A node is a processing unit.
 *
 * It contains an ID, used as a global reference, a vector of input ports, and a vector of output
 * ports.
 */
pub struct Node {
    id: NodeID,
    in_ports: Vec<InPort>,
    out_ports: Vec<OutPort>,
    attached: AtomicBool,
}

impl Node {
    /**
     * Construct a new `Node` with the given ID, number of input ports, and number of output ports.
     */
    pub(crate) fn new(id: NodeID, num_in: usize, num_out: usize) -> Node {
        Node {
            id,
            in_ports: vec![InPort::default(); num_in],
            out_ports: vec![OutPort::default(); num_out],
            attached: AtomicBool::new(false),
        }
    }

    /**
     * Does this node have any input ports?
     */
    pub fn has_inputs(&self) -> bool {
        !self.in_ports.is_empty()
    }

    /**
     * Does this node have any output ports?
     */
    pub fn has_outputs(&self) -> bool {
        !self.out_ports.is_empty()
    }

    /**
     * Returns the type of this node.
     */
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

    /**
     * Returns an array containing all input ports of this node, indexed by id.
     */
    pub fn in_ports(&self) -> &[InPort] {
        &self.in_ports
    }

    /**
     * Returns an array containing all output ports of this node, indexed by id.
     */
    pub fn out_ports(&self) -> &[OutPort] {
        &self.out_ports
    }

    /**
     * Gets an input port by id.
     */
    pub fn in_port(&self, port_id: InPortID) -> &InPort {
        &self.in_ports[port_id.0]
    }

    /**
     * Gets an output port by id.
     */
    pub fn out_port(&self, port_id: OutPortID) -> &OutPort {
        &self.out_ports[port_id.0]
    }

    /**
     * Returns the id of this node.
     */
    pub fn id(&self) -> NodeID {
        self.id
    }

    pub(crate) fn attach_thread(&self) -> Result<(), ()> {
        if self.attached
            .compare_and_swap(false, true, Ordering::SeqCst)
        {
            Err(())
        } else {
            Ok(())
        }
    }
    pub(crate) fn detach_thread(&self) -> Result<(), ()> {
        if self.attached
            .compare_and_swap(true, false, Ordering::SeqCst)
        {
            Ok(())
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
 * An `InEdge` is like a vector pointing to a specific input port of a specific node, originating from
 * nowhere in particular.
 */
#[derive(Copy, Clone, Debug)]
pub struct InEdge {
    /// Destination node
    pub node: NodeID,
    /// Destination port
    pub port: InPortID,
}
impl InEdge {
    /// Construct a new `InEdge`
    pub fn new(node: NodeID, port: InPortID) -> InEdge {
        InEdge { node, port }
    }
}

/**
 * An `OutEdge` is like a vector pointing to a specific output port of a specific node, originating from
 * nowhere in particular.
 */
#[derive(Copy, Clone, Debug)]
pub struct OutEdge {
    /// Destination node
    pub node: NodeID,
    /// Destination port
    pub port: OutPortID,
}
impl OutEdge {
    /// Construct a new `OutEdge`
    pub fn new(node: NodeID, port: OutPortID) -> OutEdge {
        OutEdge { node, port }
    }
}

pub(crate) trait Port {
    type ID;
    type Edge;
    fn new(Option<Self::Edge>) -> Self;
    fn edge(&self) -> Option<Self::Edge>;
    fn set_edge(&self, edge: Option<Self::Edge>);
    fn has_edge(&self) -> bool {
        self.edge().is_some()
    }
}

/**
 * An `InPort` can recieve data from a connected `OutPort`.
 */
#[derive(Debug)]
pub struct InPort {
    edge: Mutex<Option<OutEdge>>,
    pub(crate) data: Mutex<Vec<u8>>,
}

impl Port for InPort {
    type ID = InPortID;
    type Edge = OutEdge;

    fn new(edge: Option<OutEdge>) -> InPort {
        InPort {
            edge: Mutex::new(edge),
            data: Mutex::new(Vec::new()),
        }
    }
    fn edge(&self) -> Option<Self::Edge> {
        *self.edge.lock().unwrap()
    }
    fn set_edge(&self, edge: Option<Self::Edge>) {
        *self.edge.lock().unwrap() = edge;
    }
}

impl Clone for InPort {
    fn clone(&self) -> Self {
        InPort::new(self.edge())
    }
}
impl Default for InPort {
    fn default() -> InPort {
        InPort::new(None)
    }
}

/**
 * An `OutPort` can send data to a connected `InPort`.
 */
#[derive(Debug)]
pub struct OutPort {
    edge: Mutex<Option<InEdge>>,
}

impl Port for OutPort {
    type ID = OutPortID;
    type Edge = InEdge;
    fn new(edge: Option<InEdge>) -> OutPort {
        OutPort {
            edge: Mutex::new(edge),
        }
    }
    fn edge(&self) -> Option<Self::Edge> {
        *self.edge.lock().unwrap()
    }
    fn set_edge(&self, edge: Option<Self::Edge>) {
        *self.edge.lock().unwrap() = edge;
    }
}

impl Default for OutPort {
    fn default() -> OutPort {
        OutPort::new(None)
    }
}
impl Clone for OutPort {
    fn clone(&self) -> Self {
        OutPort::new(self.edge())
    }
}
