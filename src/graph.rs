use std::sync::{Mutex, MutexGuard, RwLock, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::VecDeque;
use std::thread::{self, Thread};
use std::boxed::FnBox;

#[derive(Debug)]
pub enum Error {
    InvalidNode,
    InvalidPort,
    Attached,
    NotAttached,
    NotConnected,
    SizeMismatch,
    Conversion,
    Aborted,
    Unavailable,
}

pub type Result<T> = ::std::result::Result<T, Error>;

/**
 * A graph contains many `Node`s and connections between their `Port`s.
 */
pub struct Graph {
    nodes: RwLock<Vec<Arc<Node>>>,
}

impl Graph {
    /**
     * Create a new empty graph.
     */
    pub fn new() -> Graph {
        Graph {
            nodes: RwLock::new(Vec::new()),
        }
    }

    /**
     * Gets the number of nodes in the graph.
     */
    pub fn num_nodes(&self) -> usize {
        self.nodes.read().unwrap().len()
    }

    /**
     * Get a node by ID.
     */
    pub fn node(&self, id: NodeID) -> Result<Arc<Node>> {
        self.nodes.read().unwrap().get(id.0).cloned().ok_or(Error::InvalidNode)
    }

    /**
     * Add a node with a fixed number of input and output ports.
     * The ID of the newly created node is returned.
     */
    pub fn add_node(&self, num_in: usize, num_out: usize) -> NodeID {
        let mut nodes = self.nodes.write().unwrap();
        let id = NodeID(nodes.len());
        nodes.push(Arc::new(Node::new(id, num_in, num_out)));
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
    ) -> Result<()> {
        let dst_node = self.node(dst_id)?;
        let in_port = dst_node.in_port(dst_port)?;
        let src_node = self.node(src_id)?;
        let out_port = src_node.out_port(src_port)?;
        if in_port.has_edge() || out_port.has_edge() {
            Err(Error::NotConnected)
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
    pub fn connect_all(&self, src_id: NodeID, dst_id: NodeID) -> Result<()> {
        let n = self.node(src_id)?.out_ports().len();
        if n != self.node(dst_id)?.in_ports().len() {
            return Err(Error::SizeMismatch);
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
    ) -> Result<()> {
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
    pub fn disconnect(&self, node_id: NodeID, port_id: InPortID) -> Result<OutEdge> {
        let node = self.node(node_id)?;
        let in_port = node.in_port(port_id)?;
        if let Some(edge) = in_port.edge() {
            in_port.set_edge(None);
            self.node(edge.node)?.out_port(edge.port)?.set_edge(None);
            Ok(edge)
        } else {
            Err(Error::NotConnected)
        }
    }

    pub fn abort(&self, node_id: NodeID) -> Result<()> {
        self.node(node_id)?.set_aborting(true);
        Ok(())
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
    in_ports: RwLock<Vec<Arc<InPort>>>,
    out_ports: RwLock<Vec<Arc<OutPort>>>,
    subs: Mutex<Vec<Thread>>,
    attached: AtomicBool,
    abort: AtomicBool,
}

impl Node {
    /**
     * Construct a new `Node` with the given ID, number of input ports, and number of output ports.
     */
    pub(crate) fn new(id: NodeID, num_in: usize, num_out: usize) -> Node {
        Node {
            id,
            in_ports: RwLock::new((0..num_in).map(|id| Arc::new(Port::new(InPortID(id), None))).collect()),
            out_ports: RwLock::new((0..num_out).map(|id| Arc::new(Port::new(OutPortID(id), None))).collect()),
            subs: Mutex::new(Vec::new()),
            attached: AtomicBool::new(false),
            abort: AtomicBool::new(false),
        }
    }

    /**
     * Does this node have any input ports?
     */
    pub fn has_inputs(&self) -> bool {
        !self.in_ports.read().unwrap().is_empty()
    }

    /**
     * Does this node have any output ports?
     */
    pub fn has_outputs(&self) -> bool {
        !self.out_ports.read().unwrap().is_empty()
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
        } else {
            // if self.has_inputs()
            NodeType::Sink
        }
    }

    /**
     * Returns an array containing all input ports of this node, indexed by id.
     */
    pub fn in_ports(&self) -> Vec<Arc<InPort>> {
        self.in_ports.read().unwrap().clone()
    }

    /**
     * Returns an array containing all output ports of this node, indexed by id.
     */
    pub fn out_ports(&self) -> Vec<Arc<OutPort>> {
        self.out_ports.read().unwrap().clone()
    }

    /**
     * Pushes a new input port to the end of the port list.
     */
    pub fn push_in_port(&self) -> Arc<InPort> {
        let mut ports = self.in_ports.write().unwrap();
        let port = Arc::new(InPort::new(InPortID(ports.len()), None));
        ports.push(port.clone());
        port
    }

    /**
     * Pushes a new output port to the end of the port list.
     */
    pub fn push_out_port(&self) -> Arc<OutPort> {
        let mut ports = self.out_ports.write().unwrap();
        let port = Arc::new(OutPort::new(OutPortID(ports.len()), None));
        ports.push(port.clone());
        port
    }

    /**
     * Removes the input port from the end of the port list.
     */
    pub fn pop_in_port(&self) {
        let mut ports = self.in_ports.write().unwrap();
        if ports.pop().and_then(|port| port.edge()).is_some() {
            panic!("disconnect before popping");
        }
    }

    /**
     * Removes the output port from the end of the port list.
     */
    pub fn pop_out_port(&self) {
        let mut ports = self.out_ports.write().unwrap();
        if ports.pop().and_then(|port| port.edge()).is_some() {
            panic!("disconnect before popping");
        }
    }

    /**
     * Gets an input port by id.
     */
    pub fn in_port(&self, port_id: InPortID) -> Result<Arc<InPort>> {
        self.in_ports.read().unwrap().get(port_id.0).cloned().ok_or(Error::InvalidPort)
    }

    /**
     * Gets an output port by id.
     */
    pub fn out_port(&self, port_id: OutPortID) -> Result<Arc<OutPort>> {
        self.out_ports.read().unwrap().get(port_id.0).cloned().ok_or(Error::InvalidPort)
    }

    /**
     * Returns the id of this node.
     */
    pub fn id(&self) -> NodeID {
        self.id
    }

    pub fn set_aborting(&self, abort: bool) {
        self.abort.store(abort, Ordering::Release);
        self.notify();
    }

    pub fn aborting(&self) -> bool {
        self.abort.load(Ordering::Acquire)
    }

    pub fn subscribe(&self) {
        self.subs.lock().unwrap().push(thread::current());
    }

    pub fn unsubscribe(&self) {
        self.subs.lock().unwrap().retain(|x| x.id() != thread::current().id());
    }

    pub fn notify(&self) {
        for thr in self.subs.lock().unwrap().iter() {
            thr.unpark();
        }
    }

    pub(crate) fn attach_thread(&self) -> Result<()> {
        if self.attached.compare_and_swap(false, true, Ordering::SeqCst) {
            Err(Error::Attached)
        } else {
            Ok(())
        }
    }
    pub(crate) fn detach_thread(&self) -> Result<()> {
        if self.attached.compare_and_swap(true, false, Ordering::SeqCst) {
            Ok(())
        } else {
            Err(Error::NotAttached)
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
 * An `InEdge` is like a vector pointing to a specific input port of a specific node, originating
 * from nowhere in particular.
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
 * An `OutEdge` is like a vector pointing to a specific output port of a specific node, originating
 * from nowhere in particular.
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

/**
 * Generic interface for a port, implemented by both `InPort` and `OutPort`.
 */
pub trait Port {
    /// The associated port ID type.
    type ID;
    /// The associated edge type.
    type Edge;

    /**
     * Construct a new port with a given ID and edge.
     */
    fn new(Self::ID, Option<Self::Edge>) -> Self;

    /**
     * Get the edge currently associated with this port, if any.
     */
    fn edge(&self) -> Option<Self::Edge>;

    /**
     * Set the edge currently associated with this port.
     */
    fn set_edge(&self, edge: Option<Self::Edge>);

    /**
     * Returns true iff this port has an edge.
     */
    fn has_edge(&self) -> bool {
        self.edge().is_some()
    }

    /**
     * Returns the ID associated with this port.
     */
    fn id(&self) -> Self::ID;

    /**
     * Returns the name of this port.
     */
    fn name(&self) -> String;

    /**
     * Sets the name of this port.
     */
    fn set_name(&self, name: String);

    /**
     * Subscribe to notifications on this port.
     * Calls to `notify` will call `unpark` on all subscribed threads.
     */
    fn subscribe(&self);

    /**
     * Unsubscribe from notifications on this port.
     */
    fn unsubscribe(&self);

    /**
     * `unpark`s all subscribed threads.
     */
    fn notify(&self);
}

/**
 * An `InPort` can recieve data from a connected `OutPort`.
 */
#[derive(Debug)]
pub struct InPort {
    edge: Mutex<Option<OutEdge>>,
    pub(crate) data: Mutex<VecDeque<u8>>,
    id: InPortID,
    name: Mutex<String>,
    subs: Mutex<Vec<Thread>>,
}

impl InPort {
    /// Safe as long as you are holding the graph lock
    pub(crate) fn data(&self) -> MutexGuard<VecDeque<u8>> {
        self.data.lock().unwrap()
    }
}
unsafe impl Sync for InPort {}

impl Port for InPort {
    type ID = InPortID;
    type Edge = OutEdge;

    fn new(id: InPortID, edge: Option<OutEdge>) -> InPort {
        InPort {
            edge: Mutex::new(edge),
            data: Mutex::new(VecDeque::new()),
            id,
            name: Mutex::new("unnamed".into()),
            subs: Mutex::new(Vec::new()),
        }
    }
    fn edge(&self) -> Option<Self::Edge> {
        *self.edge.lock().unwrap()
    }
    fn set_edge(&self, edge: Option<Self::Edge>) {
        *self.edge.lock().unwrap() = edge;
    }
    fn id(&self) -> Self::ID {
        self.id
    }
    fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }
    fn set_name(&self, name: String) {
        *self.name.lock().unwrap() = name;
    }
    fn subscribe(&self) {
        self.subs.lock().unwrap().push(thread::current());
    }
    fn unsubscribe(&self) {
        self.subs.lock().unwrap().retain(|x| x.id() != thread::current().id());
    }
    fn notify(&self) {
        for thr in self.subs.lock().unwrap().iter() {
            thr.unpark();
        }
    }
}

/**
 * An `OutPort` can send data to a connected `InPort`.
 */
#[derive(Debug)]
pub struct OutPort {
    edge: Mutex<Option<InEdge>>,
    id: OutPortID,
    name: Mutex<String>,
    subs: Mutex<Vec<Thread>>,
}

impl Port for OutPort {
    type ID = OutPortID;
    type Edge = InEdge;
    fn new(id: OutPortID, edge: Option<InEdge>) -> OutPort {
        OutPort {
            edge: Mutex::new(edge),
            id,
            name: Mutex::new("unnamed".into()),
            subs: Mutex::new(Vec::new()),
        }
    }
    fn edge(&self) -> Option<Self::Edge> {
        *self.edge.lock().unwrap()
    }
    fn set_edge(&self, edge: Option<Self::Edge>) {
        *self.edge.lock().unwrap() = edge;
    }
    fn id(&self) -> Self::ID {
        self.id
    }

    // this stuff is just copy paste from above, TODO factor
    fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }
    fn set_name(&self, name: String) {
        *self.name.lock().unwrap() = name;
    }
    fn subscribe(&self) {
        self.subs.lock().unwrap().push(thread::current());
    }
    fn unsubscribe(&self) {
        self.subs.lock().unwrap().retain(|x| x.id() != thread::current().id());
    }
    fn notify(&self) {
        for thr in self.subs.lock().unwrap().iter() {
            thr.unpark();
        }
    }
}
