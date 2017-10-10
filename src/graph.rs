use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::VecDeque;
use std::thread::{self, Thread};
use serde::ser::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug)]
pub enum Error {
    InvalidNode,
    InvalidPort,
    Attached,
    NotAttached,
    Connected,
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
#[derive(Serialize, Deserialize)]
pub struct Graph {
    id_counter: Mutex<usize>,
    nodes: RwLock<Vec<Arc<Node>>>,
}

impl Graph {
    /**
     * Create a new empty graph.
     */
    pub fn new() -> Graph {
        Graph {
            id_counter: Mutex::new(0),
            nodes: RwLock::new(Vec::new()),
        }
    }

    /**
     * Gets the number of nodes in the graph.
     */
    pub fn num_nodes(&self) -> usize {
        self.nodes().len()
    }

    /**
     * Get a node by ID.
     */
    pub fn node(&self, id: NodeID) -> Result<Arc<Node>> {
        //TODO use a more appropriate data structure
        self.nodes().iter().find(|node| node.id() == id).cloned().ok_or(Error::InvalidNode)
    }

    /**
     * Get a vector of all nodes.
     */
    pub fn nodes(&self) -> Vec<Arc<Node>> {
        self.nodes.read().unwrap().clone()
    }

    /**
     * Add a node with a fixed number of input and output ports.
     * The ID of the newly created node is returned.
     */
    pub fn add_node(&self, num_in: usize, num_out: usize) -> NodeID {
        let mut nodes = self.nodes.write().unwrap();
        let mut id_counter = self.id_counter.lock().unwrap();
        let id = NodeID(*id_counter);
        *id_counter += 1;
        nodes.push(Arc::new(Node::new(id, num_in, num_out)));
        id
    }

    pub fn remove_node(&self, id: NodeID) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        let pos = nodes.iter().position(|node| node.id() == id).ok_or(Error::InvalidNode)?;
        let node = nodes.swap_remove(pos);
        drop(nodes); // prevent deadlock because clear_ports also writes to nodes
        node.clear_ports(self);
        Ok(())
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
            Err(Error::Connected)
        } else {
            in_port.set_edge(Some(Edge::new(src_node.id(), out_port.id())));
            out_port.set_edge(Some(Edge::new(dst_node.id(), in_port.id())));
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

    pub fn disconnect_in(&self, port: Arc<InPort>) -> Result<()> {
        if let Some(edge) = port.edge() {
            self.node(edge.node)?.out_port(edge.port)?.set_edge(None);
            port.set_edge(None);
            Ok(())
        } else {
            Err(Error::NotConnected)
        }
    }
    pub fn disconnect_out(&self, port: Arc<OutPort>) -> Result<()> {
        if let Some(edge) = port.edge() {
            self.node(edge.node)?.in_port(edge.port)?.set_edge(None);
            port.set_edge(None);
            Ok(())
        } else {
            Err(Error::NotConnected)
        }
    }
}

/// A unique identifier of any `Node` in a specific `Graph`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeID(pub usize);

/// A unique identifier of any `InPort` in a specific `Node`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InPortID(pub usize);

/// A unique identifier of any `OutPort` in a specific `Node`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutPortID(pub usize);

/**
 * A node is a processing unit.
 *
 * It contains an ID, used as a global reference, a vector of input ports, and a vector of output
 * ports.
 */
#[derive(Debug, Serialize, Deserialize)]
pub struct Node {
    id: NodeID,
    in_ports: RwLock<Vec<Arc<InPort>>>,
    out_ports: RwLock<Vec<Arc<OutPort>>>,
    #[serde(skip)]
    subs: Mutex<Vec<Thread>>,
    #[serde(skip)]
    attached: AtomicBool,
    #[serde(skip)]
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
    pub fn pop_in_port(&self, graph: &Graph) {
        let mut ports = self.in_ports.write().unwrap();
        ports.pop().map(|port| graph.disconnect_in(port));
    }

    /**
     * Removes the output port from the end of the port list.
     */
    pub fn pop_out_port(&self, graph: &Graph) {
        let mut ports = self.out_ports.write().unwrap();
        ports.pop().map(|port| graph.disconnect_out(port));
    }

    pub fn clear_ports(&self, graph: &Graph) {
        {
            let mut ports = self.out_ports.write().unwrap();
            let old_ports: Vec<_> = ports.drain(..).collect();
            drop(ports);
            for port in old_ports {
                let _ = graph.disconnect_out(port);
            }
        }
        {
            let mut ports = self.in_ports.write().unwrap();
            let old_ports: Vec<_> = ports.drain(..).collect();
            drop(ports);
            for port in old_ports {
                let _ = graph.disconnect_in(port);
            }
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
        let mut subs = self.subs.lock().unwrap();
        subs.iter().position(|x| x.id() == thread::current().id()).map(|idx| subs.swap_remove(idx));
    }

    pub fn notify(&self) {
        for thr in self.subs.lock().unwrap().iter() {
            thr.unpark();
        }
    }

    pub fn attached(&self) -> bool {
        self.attached.load(Ordering::SeqCst)
    }

    pub(crate) fn attach_thread(&self) -> Result<()> {
        if self.attached.compare_and_swap(false, true, Ordering::SeqCst) {
            self.notify();
            Err(Error::Attached)
        } else {
            Ok(())
        }
    }
    pub(crate) fn detach_thread(&self) -> Result<()> {
        if self.attached.compare_and_swap(true, false, Ordering::SeqCst) {
            self.notify();
            Ok(())
        } else {
            Err(Error::NotAttached)
        }
    }

    pub fn flush(&self) -> Result<()> {
        self.attach_thread()?;
        for port in self.in_ports() {
            port.details().data().clear();
        }
        self.detach_thread()?;
        Ok(())
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
 * An `Edge` is like a vector pointing to a specific port of a specific node, originating
 * from nowhere in particular.
 */
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge<ID: Copy + Clone> {
    /// Destination node
    pub node: NodeID,
    /// Destination port
    pub port: ID,
}
impl<ID: Copy + Clone> Edge<ID> {
    /// Construct a new `Edge`
    pub fn new(node: NodeID, port: ID) -> Self {
        Edge { node, port }
    }
}

pub trait PortDetails : Default {
    type ID : Copy + Clone + Serialize + DeserializeOwned;
    type Edge : Clone + Serialize + DeserializeOwned;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Port<Details: PortDetails> {
    id: Details::ID,
    edge: Mutex<Option<Details::Edge>>,
    name: Mutex<String>,
    details: Details,
}
impl<Details: PortDetails> Port<Details> {
    /**
     * Construct a new port with a given ID and edge.
     */
    pub fn new(id: Details::ID, edge: Option<Details::Edge>) -> Self {
        Port {
            id,
            edge: Mutex::new(edge),
            name: Mutex::new("".into()),
            details: Details::default(),
        }
    }
    /**
     * Get the edge currently associated with this port, if any.
     */
    pub fn edge(&self) -> Option<Details::Edge> {
        self.edge.lock().unwrap().clone()
    }
    /**
     * Set the edge currently associated with this port.
     */
    pub fn set_edge(&self, edge: Option<Details::Edge>) {
        *self.edge.lock().unwrap() = edge;
    }
    /**
     * Returns true iff this port has an edge.
     */
    fn has_edge(&self) -> bool {
        self.edge().is_some()
    }
    /**
     * Returns the ID associated with this port.
     */
    pub fn id(&self) -> Details::ID {
        self.id
    }
    /**
     * Returns the name of this port.
     */
    pub fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }
    /**
     * Sets the name of this port.
     */
    pub fn set_name(&self, name: String) {
        *self.name.lock().unwrap() = name;
    }

    pub(crate) fn details(&self) -> &Details {
        &self.details
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Input {
    #[serde(skip)]
    pub(crate) data: Mutex<VecDeque<u8>>,
}
impl PortDetails for Input {
    type ID = InPortID;
    type Edge = Edge<OutPortID>;
}
impl Input {
    pub(crate) fn data(&self) -> MutexGuard<VecDeque<u8>> {
        self.data.lock().unwrap()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Output;
impl PortDetails for Output {
    type ID = OutPortID;
    type Edge = Edge<InPortID>;
}

pub type InEdge = Edge<Input>;
pub type OutEdge = Edge<Output>;
pub type InPort = Port<Input>;
pub type OutPort = Port<Output>;
