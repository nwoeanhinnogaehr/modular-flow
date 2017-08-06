use std::sync::{Mutex, Condvar, Arc};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};


// TODO deal with variadic mess

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
    attached: AtomicBool,
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
            in_ports: vec![InPort::default(); num_in],
            out_ports: vec![OutPort::default(); num_out],
            attached: AtomicBool::new(false),
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
    pub fn in_ports(&self) -> &[InPort] {
        &self.in_ports
    }
    pub fn out_ports(&self) -> &[OutPort] {
        &self.out_ports
    }
    pub fn in_port(&self, port_id: InPortID) -> &InPort {
        &self.in_ports[port_id.0]
    }
    pub fn out_port(&self, port_id: OutPortID) -> &OutPort {
        &self.out_ports[port_id.0]
    }
    pub fn id(&self) -> NodeID {
        self.id
    }
    pub fn attach_thread(&self) -> Result<(), ()> {
        println!("attach thread {:?}", self.id());
        if self.attached.compare_and_swap(false, true, Ordering::SeqCst) {
            Err(())
        } else {
            Ok(())
        }
    }
    pub fn detach_thread(&self) -> Result<(), ()> {
        println!("detach thread {:?}", self.id());
        if self.attached.compare_and_swap(true, false, Ordering::SeqCst) {
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

pub trait Port {
    type ID;
    type Edge;
    fn new(Option<Self::Edge>) -> Self;
    fn edge(&self) -> Option<Self::Edge>;
    fn set_edge(&self, edge: Option<Self::Edge>);
    fn has_edge(&self) -> bool {
        self.edge().is_some()
    }
}

#[derive(Debug)]
pub struct InPort {
    pub edge: Cell<Option<OutEdge>>,
    pub lock: CondvarCell<bool>,
    pub data: RefCell<Vec<u8>>,
}

impl Port for InPort {
    type ID = InPortID;
    type Edge = OutEdge;

    fn new(edge: Option<OutEdge>) -> InPort {
        InPort {
            edge: Cell::new(edge),
            lock: CondvarCell::new(false),
            data: RefCell::new(Vec::new()),
        }
    }
    fn edge(&self) -> Option<Self::Edge> {
        self.edge.get()
    }
    fn set_edge(&self, edge: Option<Self::Edge>) {
        self.edge.set(edge);
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

#[derive(Debug)]
pub struct OutPort {
    edge: Cell<Option<InEdge>>,
}

impl Port for OutPort {
    type ID = OutPortID;
    type Edge = InEdge;
    fn new(edge: Option<InEdge>) -> OutPort {
        OutPort {
            edge: Cell::new(edge),
        }
    }
    fn edge(&self) -> Option<Self::Edge> {
        self.edge.get()
    }
    fn set_edge(&self, edge: Option<Self::Edge>) {
        self.edge.set(edge);
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
