use super::graph::*;
use std::sync::{Arc, MutexGuard};
use std::mem;
use std::slice;
use std::marker::PhantomData;
use std::ops::Deref;
use std::thread;
use std::sync::atomic::Ordering;
use std::cell::Cell;

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

pub struct NodeLock<'a> {
    locked: Vec<MutexGuard<'a, Vec<u8>>>,
}

impl<'a> NodeLock<'a> {
    pub fn wait<F>(&self, cond: F)
    where
        F: FnMut(),
    {
        unimplemented!();
    }
    pub fn write<T>(&self, data: Data<T>) {
        unimplemented!();
    }
    pub fn read<T>(&self) -> Data<T> {
        unimplemented!();
    }
    pub fn read_n<T>(&self, n: usize) -> Data<T> {
        unimplemented!();
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
    pub fn lock<'a>(&'a self) -> NodeLock<'a> {
        let node = self.sched.graph.node(self.id);
        let mut locked = Vec::new();
        for in_port in node.in_ports() {
            let lock = in_port.data.lock().unwrap();
            locked.push(lock);
        }
        for out_port in node.out_ports() {
            if let Some(edge) = out_port.edge() {
                let dst_node = self.sched.graph.node(edge.node);
                let in_port = dst_node.in_port(edge.port);
                let lock = in_port.data.lock().unwrap();
                locked.push(lock);
            }
        }
        NodeLock { locked }
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
