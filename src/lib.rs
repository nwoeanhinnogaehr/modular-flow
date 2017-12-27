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

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub usize);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PortId(pub usize);

pub struct Graph<Module> {
    nodes: RwLock<HashMap<NodeId, Arc<Node<Module>>>>,
    id_counter: AtomicUsize,
}

impl<Module> Graph<Module> {
    pub fn new() -> Arc<Graph<Module>> {
        Arc::new(Graph {
            nodes: RwLock::new(HashMap::new()),
            id_counter: 0.into(),
        })
    }
    pub fn add_node(self: &Arc<Graph<Module>>, meta: &MetaModule<Module>) -> Arc<Node<Module>> {
        let node = Node::new(self, meta);
        self.nodes.write().unwrap().insert(node.id(), node.clone());
        node
    }
    pub fn remove_node(&self, node: NodeId) -> Result<Arc<Node<Module>>, Error> {
        self.nodes
            .write()
            .unwrap()
            .remove(&node)
            .ok_or(Error::InvalidNode)
    }
    pub fn nodes(&self) -> Vec<Arc<Node<Module>>> {
        self.nodes.read().unwrap().values().cloned().collect()
    }
    pub fn node_map(&self) -> HashMap<NodeId, Arc<Node<Module>>> {
        self.nodes.read().unwrap().clone()
    }

    fn generate_id(&self) -> usize {
        self.id_counter.fetch_add(1, Ordering::SeqCst)
    }
}

pub struct Node<Module> {
    ifc: Arc<Interface<Module>>,
    module: Module,
}

impl<Module> Node<Module> {
    fn new(graph: &Arc<Graph<Module>>, meta: &MetaModule<Module>) -> Arc<Node<Module>> {
        let ifc = Arc::new(Interface::new(graph, MetaModule::clone(meta)));
        let module = (meta.new)(ifc.clone());
        Arc::new(Node {
            ifc,
            module,
        })
    }

    pub fn module(&self) -> &Module {
        &self.module
    }
    pub fn meta(&self) -> &MetaModule<Module> {
        self.ifc.meta()
    }

    pub fn id(&self) -> NodeId {
        self.ifc.id()
    }
    pub fn find_port(&self, name: &'static str) -> Arc<Port> {
        self.ifc.find_port(name)
    }
    pub fn ports(&self) -> Vec<Arc<Port>> {
        self.ifc.ports()
    }
}

pub struct Interface<Module> {
    id: NodeId,
    ports: RwLock<HashMap<PortId, Arc<Port>>>,
    graph: Weak<Graph<Module>>,
    meta: MetaModule<Module>,
}

impl<Module> Interface<Module> {
    fn new(graph: &Arc<Graph<Module>>, meta: MetaModule<Module>) -> Interface<Module> {
        Interface {
            id: NodeId(graph.generate_id()),
            ports: RwLock::new(HashMap::new()),
            graph: Arc::downgrade(graph),
            meta,
        }
    }

    pub fn meta(&self) -> &MetaModule<Module> {
        &self.meta
    }
    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn find_port(&self, name: &str) -> Arc<Port> {
        self.ports
            .read()
            .unwrap()
            .iter()
            .find(|&(_, port)| port.meta.name == name)
            .unwrap()
            .1
            .clone()
    }
    pub fn ports(&self) -> Vec<Arc<Port>> {
        self.ports.read().unwrap().values().cloned().collect()
    }
    pub fn add_port(&self, meta: &MetaPort) -> Arc<Port> {
        let port = Port::new(&self.graph.upgrade().unwrap(), meta);
        self.ports.write().unwrap().insert(port.id, port.clone());
        port
    }
    pub fn remove_port(&self, port: PortId) -> Result<Arc<Port>, Error> {
        self.ports
            .write()
            .unwrap()
            .remove(&port)
            .ok_or(Error::InvalidPort)
    }

    pub fn wait(&self, port: &Port) -> Signal {
        port.wait()
    }
    pub fn poll(&self, port: &Port) -> Vec<Signal> {
        port.poll()
    }
    pub fn write<T: 'static>(&self, port: &Port, data: impl Into<Box<[T]>>) -> Result<(), Error> {
        port.write(data.into())
    }
    pub fn read<T: 'static>(&self, port: &Port) -> Result<Box<[T]>, Error> {
        port.read()
    }
    pub fn read_n<T: 'static>(&self, port: &Port, n: usize) -> Result<Box<[T]>, Error> {
        port.read_n(n)
    }
}

pub struct MetaModule<Module> {
    pub name: Cow<'static, str>,
    pub new: Arc<Fn(Arc<Interface<Module>>) -> Module>,
}

impl<Module> MetaModule<Module> {
    pub fn new(name: impl Into<Cow<'static, str>>,
               new: Arc<Fn(Arc<Interface<Module>>) -> Module>)-> MetaModule<Module> {
        MetaModule {
            name: name.into(),
            new: new,
        }
    }
}

// need manual impl because derive doesn't play nice with generics
impl<T> Clone for MetaModule<T> {
    fn clone(&self) -> Self {
        MetaModule {
            name: self.name.clone(),
            new: self.new.clone(),
        }
    }
}

#[derive(Clone)]
pub struct MetaPort {
    name: Cow<'static, str>,
    ty: TypeId,
}

impl MetaPort {
    pub fn new<T: 'static, N: Into<Cow<'static, str>>>(name: N) -> MetaPort {
        MetaPort {
            name: name.into(),
            ty: TypeId::of::<T>(),
        }
    }
}

pub struct Port {
    meta: MetaPort,
    id: PortId,
    buffer: Mutex<VecDeque<u8>>,
    edge: RwLock<Option<Weak<Port>>>,
    signal: Mutex<VecDeque<Signal>>,
    cvar: Condvar,
}

impl Port {
    fn new<Module>(graph: &Graph<Module>, meta: &MetaPort) -> Arc<Port> {
        Arc::new(Port {
            meta: MetaPort::clone(meta),
            id: PortId(graph.generate_id()),
            buffer: Mutex::new(VecDeque::new()),
            edge: RwLock::new(None),
            signal: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
        })
    }

    pub fn meta(&self) -> &MetaPort {
        &self.meta
    }
    pub fn connect(self: Arc<Port>, other: Arc<Port>) {
        assert!(self.meta.ty == other.meta.ty);
        self.set_edge(Some(&other));
        other.set_edge(Some(&self));
        self.signal(Signal::Connect);
        other.signal(Signal::Connect);
    }
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

#[derive(Debug)]
pub enum Error {
    NotConnected,
    InvalidNode,
    InvalidPort,
    NotAvailable,
}

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
