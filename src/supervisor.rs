use super::graph::*;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::mem;
use std::slice;
use std::marker::PhantomData;
use std::ops::Deref;
use std::thread;
use std::sync::atomic::Ordering;
use std::cell::{Cell, RefCell};

pub struct Data<T> {
    data: Vec<T>,
}

impl<T> Data<T> {
    pub fn new(data: Vec<T>) -> Data<T> {
        Data { data }
    }
}

impl<T> Deref for Data<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub struct NodeGuard<'a> {
    node: &'a Node,
    sched: &'a Scheduler,
    guard: Option<MutexGuard<'a, ()>>,
}

impl<'a> NodeGuard<'a> {
    fn new(sched: &'a Scheduler, node_id: NodeID) -> NodeGuard<'a> {
        let node = sched.graph.node(node_id);
        let guard = Some(sched.graph.lock.lock().unwrap());
        NodeGuard { node, sched, guard }
    }
    pub fn wait<F>(&mut self, mut cond: F)
    where
        F: FnMut(&Self) -> bool,
    {
        let mut guard = self.guard.take().unwrap();
        while !cond(self) {
            guard = self.sched.graph.cond.wait(guard).unwrap();
        }
        self.guard = Some(guard);
    }
    pub fn write(&mut self, port: OutPortID, data: &[u8]) {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.sched.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let mut buffer = in_port.data.lock().unwrap();
        buffer.extend(data.iter());
        self.sched.graph.cond.notify_all();
    }
    pub fn read(&mut self, port: InPortID) -> Data<u8> {
        let in_port = self.node.in_port(port);
        let mut buffer = in_port.data.lock().unwrap();
        let out = buffer.drain(..).collect();
        self.sched.graph.cond.notify_all();
        Data::new(out)
    }
    pub fn read_n(&mut self, port: InPortID, n: usize) -> Data<u8> {
        let in_port = self.node.in_port(port);
        let mut buffer = in_port.data.lock().unwrap();
        if buffer.len() < n {
            return Data::new(Vec::new());
        }
        let out = buffer.drain(..n).collect();
        self.sched.graph.cond.notify_all();
        Data::new(out)
    }
    pub fn available(&self, port: InPortID) -> usize {
        let in_port = self.node.in_port(port);
        let buffer = in_port.data.lock().unwrap();
        buffer.len()
    }
    pub fn buffered(&self, port: OutPortID) -> usize {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.sched.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let buffer = in_port.data.lock().unwrap();
        buffer.len()
    }
    // read_while, ...
}

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext {
    id: NodeID,
    sched: Arc<Scheduler>,
}

impl NodeContext {
    pub fn lock<'a>(&'a self) -> NodeGuard<'a> {
        NodeGuard::new(&*self.sched, self.id)
    }
}

impl Drop for NodeContext {
    fn drop(&mut self) {
        self.sched.graph.node(self.id).detach_thread().unwrap();
    }
}

/**
 * The supervisor runs the main loop. It owns the graph, and manages access to the graph while the
 * application is running. Node threads must be attached to the supervisor by requesting a context.
 *
 * TODO these all gotta get renamed
 */
pub struct Supervisor {
    sched: Arc<Scheduler>,
}

impl Supervisor {
    pub fn new(graph: Graph) -> Supervisor {
        Supervisor {
            sched: Arc::new(Scheduler::new(graph)),
        }
    }
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext, ()> {
        self.sched.graph.node(node).attach_thread().map(|_| {
            NodeContext {
                sched: self.sched.clone(),
                id: node,
            }
        })
    }
    pub fn run(self) {
        thread::park();
    }
}

pub struct Scheduler {
    graph: Graph,
}


impl Scheduler {
    fn new(graph: Graph) -> Scheduler {
        Scheduler { graph }
    }
}
