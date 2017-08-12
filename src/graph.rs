use std::sync::{Condvar, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::VecDeque;

/**
 * A graph contains many `Node`s and connections between their `Port`s.
 */
pub struct Graph {
    nodes: Vec<Node>,

    // I'd like to move these out of the graph eventually but with the current architecture it's
    // most efficient to have them here.
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
     * Connects all output ports of node `src_id` to corresponding input ports of node `dst_id`.
     *
     * Returns Err if any such ports are already connected, or if the nodes have different port
     * counts.
     */
    pub fn connect_all(
        &self,
        src_id: NodeID,
        dst_id: NodeID,
    ) -> Result<(), ()> {
        let n = self.node(src_id).out_ports().len();
        if n != self.node(dst_id).in_ports().len() {
            return Err(())
        }
        for id in 0..n {
            self.connect(src_id, OutPortID(id), dst_id, InPortID(id))?;
        }
        Ok(())
    }

    /**
     * Connects `n` consecutive output ports of node `src_id`, starting with `src_port`, to `n`
     * corresponding input ports of node `dst_id`, starting with `dst_id`.
     *
     * Returns Err if any such ports are already connected.
     */
    pub fn connect_n(
        &self,
        src_id: NodeID,
        src_port: OutPortID,
        dst_id: NodeID,
        dst_port: InPortID,
        n: usize,
    ) -> Result<(), ()> {
        for id in 0..n {
            self.connect(src_id, OutPortID(src_port.0 + id), dst_id, InPortID(dst_port.0 + id))?;
        }
        Ok(())
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
        } else { // if self.has_inputs()
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
    pub(crate) data: Mutex<VecDeque<u8>>,
}

impl Port for InPort {
    type ID = InPortID;
    type Edge = OutEdge;

    fn new(edge: Option<OutEdge>) -> InPort {
        InPort {
            edge: Mutex::new(edge),
            data: Mutex::new(VecDeque::new()),
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
