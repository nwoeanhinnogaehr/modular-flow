#![feature(universal_impl_trait)]
#![feature(arbitrary_self_types)]
/*!
 * This is some kind of a library for dataflow computation. It's still very experimental and may
 * become something completely different in the end.
 *
 * The end goal is to use it for procedural and generative art. It's inspired by Pure Data and
 * Max/MSP, but will probably have less focus on graphical programming. Modular live coding,
 * perhaps?
 *
 * This is iteration #2.
 */

use std::sync::{Arc, Condvar, Mutex, RwLock, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::{HashMap, VecDeque};
use std::mem;
use std::slice;
use std::any::TypeId;
use std::borrow::Cow;

/// This trait must be implemented by the inner data type of the graph.
/// Careful: Make sure your module doesn't hold an `Arc<Node>` or cyclic references may prevent it
/// from being dropped.
pub trait Module {
    /// Argument for construction of the module.
    type Arg;
}

// HKT please?
impl<T: ?Sized> Module for Mutex<T>
where
    T: Module,
{
    type Arg = T::Arg;
}
impl<T: ?Sized> Module for Box<T>
where
    T: Module,
{
    type Arg = T::Arg;
}

/// A lightweight persistent identifier for a node.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub usize);

/// A lightweight persistent identifier for a port. Only gauranteeed to be unique within a specific
/// node.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PortId(pub usize);

/// A graph holds a collection of Nodes. Nodes have a collection of Ports. Ports can be connected
/// to each other one-to-one.
pub struct Graph<T: Module> {
    nodes: RwLock<HashMap<NodeId, Arc<Node<T>>>>,
    id_counter: AtomicUsize,
}

impl<T: Module> Graph<T> {
    /// Make a new empty graph.
    pub fn new() -> Arc<Graph<T>> {
        Arc::new(Graph {
            nodes: RwLock::new(HashMap::new()),
            id_counter: 0.into(),
        })
    }
    /// Construct a new node from the given metadata and argument.
    pub fn add_node(self: &Arc<Graph<T>>, meta: &MetaModule<T>, arg: T::Arg) -> Arc<Node<T>> {
        let node = Node::new(self, meta, arg);
        self.nodes.write().unwrap().insert(node.id(), node.clone());
        node
    }
    /// Delete a node by id.
    pub fn remove_node(&self, node: NodeId) -> Result<Arc<Node<T>>, Error> {
        self.nodes
            .write()
            .unwrap()
            .remove(&node)
            .ok_or(Error::InvalidNode)
    }
    /// Returns a vector containing references to all nodes active at the time of the call.
    pub fn nodes(&self) -> Vec<Arc<Node<T>>> {
        self.nodes.read().unwrap().values().cloned().collect()
    }
    /// Returns a hash map from id to node references for all nodes active at the time of the call.
    pub fn node_map(&self) -> HashMap<NodeId, Arc<Node<T>>> {
        self.nodes.read().unwrap().clone()
    }

    fn generate_id(&self) -> usize {
        self.id_counter.fetch_add(1, Ordering::SeqCst)
    }
}

/// A node is the public interface for generic functionality on a module in the graph.
/// It holds a `Module`.
pub struct Node<T: Module> {
    ifc: Arc<Interface<T>>,
    module: T,
}

impl<T: Module> Node<T> {
    fn new(graph: &Arc<Graph<T>>, meta: &MetaModule<T>, arg: T::Arg) -> Arc<Node<T>> {
        let ifc = Arc::new(Interface::new(graph, MetaModule::clone(meta)));
        let module = (meta.new)(ifc.clone(), arg);
        Arc::new(Node { ifc, module })
    }

    /// Get a reference to the module held by this node.
    pub fn module(&self) -> &T {
        &self.module
    }
    /// Get a reference to the metadata associated with this node.
    pub fn meta(&self) -> &MetaModule<T> {
        self.ifc.meta()
    }
    /// Get the node ID.
    pub fn id(&self) -> NodeId {
        self.ifc.id()
    }
    /// Find a port by name (name is held within the associated `MetaPort`)
    pub fn find_port(&self, name: &'static str) -> Option<Arc<Port>> {
        self.ifc.find_port(name)
    }
    /// Get a vector of references to all associated ports at the time of the call.
    pub fn ports(&self) -> Vec<Arc<Port>> {
        self.ifc.ports()
    }
}

/// The private interface for a module. The module is provided with an `Interface` upon construction.
/// An `Interface` has a superset of the functionality of a `Node`. It can be used to manipulate the
/// associated Ports.
pub struct Interface<T: Module> {
    id: NodeId,
    ports: RwLock<HashMap<PortId, Arc<Port>>>,
    graph: Weak<Graph<T>>,
    meta: MetaModule<T>,
}

impl<T: Module> Interface<T> {
    fn new(graph: &Arc<Graph<T>>, meta: MetaModule<T>) -> Interface<T> {
        Interface {
            id: NodeId(graph.generate_id()),
            ports: RwLock::new(HashMap::new()),
            graph: Arc::downgrade(graph),
            meta,
        }
    }
    /// Get a reference to the metadata associated with this node.
    pub fn meta(&self) -> &MetaModule<T> {
        &self.meta
    }
    /// Get the node ID.
    pub fn id(&self) -> NodeId {
        self.id
    }
    /// Find a port by name (name is held within the associated `MetaPort`)
    pub fn find_port(&self, name: &str) -> Option<Arc<Port>> {
        self.ports
            .read()
            .unwrap()
            .iter()
            .find(|&(_, port)| port.meta.name == name)
            .map(|port| port.1)
            .cloned()
    }
    /// Get a vector of references to all associated ports at the time of the call.
    pub fn ports(&self) -> Vec<Arc<Port>> {
        self.ports.read().unwrap().values().cloned().collect()
    }
    /// Add a new port using the given metadata.
    pub fn add_port(&self, meta: &MetaPort) -> Arc<Port> {
        let port = Port::new(&self.graph.upgrade().unwrap(), meta);
        self.ports.write().unwrap().insert(port.id, port.clone());
        port
    }
    /// Remove a port by ID.
    pub fn remove_port(&self, port: PortId) -> Result<Arc<Port>, Error> {
        self.ports
            .write()
            .unwrap()
            .remove(&port)
            .ok_or(Error::InvalidPort)
    }

    /// Wait (block) until a `Signal` is received on a port.
    pub fn wait(&self, port: &Port) -> Signal {
        port.wait()
    }
    /// Get a vector of all unread Signals on a port.
    pub fn poll(&self, port: &Port) -> Vec<Signal> {
        port.poll()
    }
    /// Write data to a port.
    pub fn write<D: 'static>(&self, port: &Port, data: impl Into<Box<[D]>>) -> Result<(), Error> {
        port.write(data.into())
    }
    /// Read all available data from a port.
    pub fn read<D: 'static>(&self, port: &Port) -> Result<Box<[D]>, Error> {
        port.read()
    }
    /// Read exactly `n` values from a port.
    pub fn read_n<D: 'static>(&self, port: &Port, n: usize) -> Result<Box<[D]>, Error> {
        port.read_n(n)
    }
}

/// Module metadata.
pub struct MetaModule<T: Module> {
    name: Cow<'static, str>,
    new: Arc<Fn(Arc<Interface<T>>, T::Arg) -> T>,
}

impl<T: Module> MetaModule<T> {
    /// Construct new module metadata with the given name and constructor.
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        new: Arc<Fn(Arc<Interface<T>>, T::Arg) -> T>,
    ) -> MetaModule<T> {
        MetaModule {
            name: name.into(),
            new: new,
        }
    }
    /// Get the module name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

// need manual impl because derive doesn't play nice with generics
impl<T: Module> Clone for MetaModule<T> {
    fn clone(&self) -> Self {
        MetaModule {
            name: self.name.clone(),
            new: self.new.clone(),
        }
    }
}

/// Port metadata.
#[derive(Clone)]
pub struct MetaPort {
    name: Cow<'static, str>,
    ty: TypeId,
}

impl MetaPort {
    /// Construct new port metadata with the given datatype and name.
    pub fn new<T: 'static, N: Into<Cow<'static, str>>>(name: N) -> MetaPort {
        MetaPort {
            name: name.into(),
            ty: TypeId::of::<T>(),
        }
    }
    /// Get the port name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Ports are the connection points of modules. They can be connected one-to-one with other ports,
/// and allow a single type of data (runtime checked) to flow bidirectionally.
///
/// TODO allow different input and output types.
pub struct Port {
    meta: MetaPort,
    id: PortId,
    buffer: Mutex<VecDeque<u8>>,
    edge: RwLock<Option<Weak<Port>>>,
    signal: Mutex<VecDeque<Signal>>,
    cvar: Condvar,
}

impl Port {
    fn new<T: Module>(graph: &Graph<T>, meta: &MetaPort) -> Arc<Port> {
        Arc::new(Port {
            meta: MetaPort::clone(meta),
            id: PortId(graph.generate_id()),
            buffer: Mutex::new(VecDeque::new()),
            edge: RwLock::new(None),
            signal: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
        })
    }

    /// Get the associated metadata.
    pub fn meta(&self) -> &MetaPort {
        &self.meta
    }
    /// Connect this port to another.
    /// TODO: think about error handling and race conditions
    pub fn connect(self: Arc<Port>, other: Arc<Port>) {
        assert!(self.meta.ty == other.meta.ty);
        self.set_edge(Some(&other));
        other.set_edge(Some(&self));
        self.signal(Signal::Connect);
        other.signal(Signal::Connect);
    }
    /// Disconnect this port from another.
    /// TODO: WTF was I thinking when I wrote this???
    pub fn disconnect(self: Arc<Port>, other: Arc<Port>) {
        self.set_edge(None);
        other.set_edge(None);
        self.signal(Signal::Disconnect);
        other.signal(Signal::Disconnect);
    }

    fn set_edge(&self, other: Option<&Arc<Port>>) {
        *self.edge.write().unwrap() = other.map(|x| Arc::downgrade(&x));
    }
    fn edge(&self) -> Option<Arc<Port>> {
        self.edge.read().unwrap().clone().and_then(|x| x.upgrade())
    }
    fn signal(&self, signal: Signal) {
        let mut lock = self.signal.lock().unwrap();
        lock.push_back(signal);
        self.cvar.notify_all();
    }
    fn poll(&self) -> Vec<Signal> {
        let mut lock = self.signal.lock().unwrap();
        let iter = lock.drain(..);
        iter.collect()
    }
    fn wait(&self) -> Signal {
        let mut lock = self.signal.lock().unwrap();
        while lock.is_empty() {
            lock = self.cvar.wait(lock).unwrap();
        }
        lock.pop_front().unwrap()
    }
    fn write<T: 'static>(&self, data: impl Into<Box<[T]>>) -> Result<(), Error> {
        assert!(self.meta.ty == TypeId::of::<T>());
        let bytes = typed_as_bytes(data.into());
        let other = self.edge().ok_or(Error::NotConnected)?;
        let mut buf = other.buffer.lock().unwrap();
        buf.extend(bytes.into_iter());
        other.signal(Signal::Write);
        Ok(())
    }
    fn read<T: 'static>(&self) -> Result<Box<[T]>, Error> {
        assert!(self.meta.ty == TypeId::of::<T>());
        let mut buf = self.buffer.lock().unwrap();
        let iter = buf.drain(..);
        let out = iter.collect::<Vec<_>>().into();
        Ok(bytes_as_typed(out))
    }
    fn read_n<T: 'static>(&self, n: usize) -> Result<Box<[T]>, Error> {
        assert!(self.meta.ty == TypeId::of::<T>());
        let mut buf = self.buffer.lock().unwrap();
        if n > buf.len() {
            return Err(Error::NotAvailable);
        }
        let iter = buf.drain(..n);
        let out = iter.collect::<Vec<_>>().into();
        Ok(bytes_as_typed(out))
    }
}

/// Error cases
#[derive(Debug)]
pub enum Error {
    NotConnected,
    InvalidNode,
    InvalidPort,
    NotAvailable,
}

/// Events occuring on a `Port`. Accessible via an `Interface`.
#[derive(Debug)]
pub enum Signal {
    Abort,
    Write,
    Connect,
    Disconnect,
}

fn typed_as_bytes<T: 'static>(data: Box<[T]>) -> Box<[u8]> {
    let size = data.len() * mem::size_of::<T>();
    let raw = Box::into_raw(data);
    unsafe { Box::from_raw(slice::from_raw_parts_mut(raw as *mut u8, size)) }
}

fn bytes_as_typed<T: 'static>(data: Box<[u8]>) -> Box<[T]> {
    assert!(data.len() % mem::size_of::<T>() == 0);
    let size = data.len() / mem::size_of::<T>();
    let raw = Box::into_raw(data);
    unsafe { Box::from_raw(slice::from_raw_parts_mut(raw as *mut T, size)) }
}
