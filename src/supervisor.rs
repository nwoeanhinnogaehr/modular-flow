use super::graph::*;
use std::sync::{Arc, Mutex, MutexGuard, Condvar};
use std::mem;
use std::slice;
use std::marker::PhantomData;
use std::ops::Deref;
use std::thread;
use std::sync::atomic::Ordering;
use std::cell::{RefCell,Cell};

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
        let guard = Some(node.lock.lock().unwrap());
        NodeGuard {
            node,
            sched,
            guard,
        }
    }
    pub fn wait<F>(&mut self, mut cond: F)
    where
        F: FnMut() -> bool,
    {
        let mut guard = self.guard.take().unwrap();
        while !cond() {
            guard = self.node.cond.wait(guard).unwrap();
        }
        self.guard = Some(guard);
    }
    pub fn write<T>(&mut self, port: OutPortID, data: Data<T>) {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.sched.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
    }
    pub fn read<T>(&mut self) -> Data<T> {
        self.node.cond.notify_one();
        unimplemented!();
    }
    pub fn read_n<T>(&mut self, n: usize) -> Data<T> {
        self.node.cond.notify_one();
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
