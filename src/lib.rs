#![feature(fnbox)]
#![feature(universal_impl_trait)]
#![feature(conservative_impl_trait)]
/*!
 * This is some kind of a library for dataflow computation. It's still very experimental and may
 * become something completely different in the end.
 *
 * The end goal is to use it for procedural and generative art. It's inspired by Pure Data and
 * Max/MSP, but will probably have less focus on graphical programming. Modular live coding,
 * perhaps?
 */

use std::sync::{Mutex, Arc, RwLock, Condvar};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::{HashMap, VecDeque};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct NodeId(usize);
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct PortId(usize);

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
    fn add_node(&self, node: Arc<Node>) {
        self.nodes.write().unwrap().insert(node.id, node);
    }
    fn remove_node(&self, node: Arc<Node>) {
        self.nodes.write().unwrap().remove(&node.id).unwrap();
    }
}

pub struct Node {
    id: NodeId,
    ports: RwLock<HashMap<PortId, Arc<Port>>>,
    module: Option<Box<Module>>,
    meta: MetaModule,
}

pub struct Port {
    id: PortId,
    buffer: Mutex<VecDeque<u8>>,
    edge: RwLock<Option<Arc<Port>>>,
    signal: Mutex<Option<Signal>>,
    cvar: Condvar,
}

impl Port {
    fn set_edge(&self, other: Option<Arc<Port>>) {
        *self.edge.write().unwrap() = other;
    }
    fn edge(&self) -> Option<Arc<Port>> {
        self.edge.read().unwrap().clone()
    }
    fn signal(&self, signal: Signal) {
        let mut lock = self.signal.lock().unwrap();
        assert!(lock.is_none()); // I think this shouldn't happen but maybe my reasoning is bad
        *lock = Some(signal);
        self.cvar.notify_all();
    }
    fn wait(&self) -> Signal {
        let mut lock = self.signal.lock().unwrap();
        while lock.is_none() {
            lock = self.cvar.wait(lock).unwrap();
        }
        lock.take().unwrap()
    }
    fn write<T>(&self, data: impl Into<Box<[T]>>) {
        unimplemented!();
    }
    fn read<T>(&self) -> Box<[T]> {
        unimplemented!();
    }
    fn read_n<T>(&self, n: usize) -> Box<[T]> {
        unimplemented!();
    }
}

enum Signal {
    Abort,
    Write,
    Read,
    Connect,
}

impl Drop for Port {
    fn drop(&mut self) {
        if let Some(other) = self.edge() {
            other.set_edge(None);
            self.set_edge(None);
        }
    }
}

pub struct Interface<'a> {
    node: &'a Node,
}

impl<'a> Interface<'a> {
    fn new(node: &'a Node) -> Interface<'a> {
        Interface {
            node,
        }
    }
    pub fn write<T>(&self, port: &Port, data: impl Into<Box<[T]>>) {
        port.write(data);
    }
    pub fn read<T>(&self, port: &Port) -> Box<[T]> {
        port.read()
    }
    pub fn read_n<T>(&self, port: &Port, n: usize) -> Box<[T]> {
        port.read_n(n)
    }
}

pub struct MetaModule {
    id: &'static str,
    new: fn(Interface) -> Box<Module>,
}

pub trait Module {
    fn start(&self);
    fn stop(&self);
}

pub fn create_module(graph: &Graph, meta: MetaModule) -> Arc<Node> {
    let mut node = Node {
        id: NodeId(graph.id_counter.fetch_add(1, Ordering::SeqCst)),
        ports: RwLock::new(HashMap::new()),
        module: None,
        meta,
    };
    let module = (node.meta.new)(Interface::new(&node));
    node.module = Some(module);
    let node = Arc::new(node);
    graph.add_node(node.clone());
    node
}

pub fn connect(a: Arc<Port>, b: Arc<Port>) {
    a.set_edge(Some(b.clone()));
    b.set_edge(Some(a.clone()));
    a.signal(Signal::Connect);
    b.signal(Signal::Connect);
}
