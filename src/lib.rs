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

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct PortId(usize);

pub struct Graph {
    nodes: RwLock<HashMap<NodeId, Arc<Node>>>,
    id_counter: AtomicUsize,
}

impl Graph {
    pub fn new() -> Graph {
        Graph {
            nodes: RwLock::new(HashMap::new()),
            id_counter: 0.into(),
        }
    }
    pub fn add_node(self: &Arc<Graph>, meta: &MetaModule) -> Arc<Node> {
        let node = Node {
            id: NodeId(self.generate_id()),
            ports: RwLock::new(HashMap::new()),
            module: Mutex::new(None),
            meta: meta.clone(),
        };
        let node = Arc::new(node);
        let module = (meta.new)(Interface::new(self, &node));
        *node.module.lock().unwrap() = Some(module);
        self.nodes.write().unwrap().insert(node.id, node.clone());
        node
    }
    pub fn remove_node(&self, node: NodeId) -> Result<Arc<Node>, Error> {
        self.nodes
            .write()
            .unwrap()
            .remove(&node)
            .ok_or(Error::InvalidNode)
    }
    pub fn nodes(&self) -> Vec<Arc<Node>> {
        self.nodes.read().unwrap().values().cloned().collect()
    }

    fn generate_id(&self) -> usize {
        self.id_counter.fetch_add(1, Ordering::SeqCst)
    }
}

pub struct Node {
    id: NodeId,
    ports: RwLock<HashMap<PortId, Arc<Port>>>,
    module: Mutex<Option<Box<Module>>>,
    meta: MetaModule,
}

impl Node {
    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn start(&self) {
        self.module.lock().unwrap().as_mut().unwrap().start();
    }
    pub fn stop(&self) {
        for port in self.ports() {
            port.signal(Signal::Abort);
        }
        self.module.lock().unwrap().as_mut().unwrap().stop();
    }
    pub fn find_port(&self, name: &'static str) -> Arc<Port> {
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

    fn add_port(&self, graph: &Graph, meta: MetaPort) -> Arc<Port> {
        let port = Port {
            meta,
            id: PortId(graph.generate_id()),
            buffer: Mutex::new(VecDeque::new()),
            edge: RwLock::new(None),
            signal: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
        };
        let port = Arc::new(port);
        self.ports.write().unwrap().insert(port.id, port.clone());
        port
    }
    fn remove_port(&self, port: PortId) -> Result<Arc<Port>, Error> {
        self.ports
            .write()
            .unwrap()
            .remove(&port)
            .ok_or(Error::InvalidPort)
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
    fn write<T: 'static>(&self, data: Box<[T]>) -> Result<(), Error> {
        assert!(self.meta.ty == TypeId::of::<T>());
        let bytes = typed_as_bytes(data);
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

#[derive(Clone)]
pub struct Interface {
    graph: Weak<Graph>,
    node: Weak<Node>,
}

impl Interface {
    fn new(graph: &Arc<Graph>, node: &Arc<Node>) -> Interface {
        Interface {
            graph: Arc::downgrade(graph),
            node: Arc::downgrade(node),
        }
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
    pub fn add_port(&self, meta: MetaPort) -> Arc<Port> {
        self.node
            .upgrade()
            .unwrap()
            .add_port(&self.graph.upgrade().unwrap(), meta)
    }
    pub fn remove_port(&self, port: PortId) -> Result<Arc<Port>, Error> {
        self.node.upgrade().unwrap().remove_port(port)
    }
    pub fn find_port(&self, name: &'static str) -> Arc<Port> {
        self.node.upgrade().unwrap().find_port(name)
    }
    pub fn wait(&self, port: &Port) -> Signal {
        port.wait()
    }
    pub fn poll(&self, port: &Port) -> Vec<Signal> {
        port.poll()
    }
}

#[derive(Clone)]
pub struct MetaModule {
    id: &'static str,
    new: fn(Interface) -> Box<Module>,
}

pub trait Module: Send + Sync {
    fn start(&mut self);
    fn stop(&mut self);
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
