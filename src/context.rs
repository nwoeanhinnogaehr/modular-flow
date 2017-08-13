use super::graph::*;
use std::sync::{Arc, MutexGuard};
use std::cell::UnsafeCell;
use std::slice;
use std::mem;
use std::fmt;
use std::ptr;
use num_complex::Complex;
use std::ops::Deref;

/**
 * An array of any type which is `ByteConvertible` can be converted to an array of bytes and back
 * again.
 */
pub trait ByteConvertible: Sized + Copy {
    /// Type returned if conversion fails.
    type Error: fmt::Debug;
    /// Converts a slice of this into a vector of bytes, returning `None` on failure.
    fn to_bytes(data: &[Self]) -> Result<Vec<u8>, Self::Error>;
    /// Converts a slice of bytes into a vector of this, returning `None` on failure.
    fn from_bytes(data: &[u8]) -> Result<Vec<Self>, Self::Error>;
}

/**
 * Any type which is `TransmuteByteConvertible` can be converted to an array of bytes by simply
 * transmuting the underlying data.
 */
pub unsafe trait TransmuteByteConvertible: Sized + Clone {
    /**
     * Always returns `Ok`.
     */
    fn to_bytes(data: &[Self]) -> Result<Vec<u8>, ()> {
        let mut out = Vec::new();
        out.extend_from_slice(unsafe {
            slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() * mem::size_of::<Self>())
        });
        Ok(out)
    }
    /**
     * Returns `Err` if the number of bytes given is not a multiple of the data size.
     */
    fn from_bytes(data: &[u8]) -> Result<Vec<Self>, ()> {
        let mut out = Vec::new();
        out.extend_from_slice(if data.len() % mem::size_of::<Self>() == 0 {
            unsafe {
                slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() / mem::size_of::<Self>())
            }
        } else {
            return Err(());
        });
        Ok(out)
    }
}

impl<T: TransmuteByteConvertible + Copy> ByteConvertible for T {
    type Error = ();
    fn to_bytes(data: &[Self]) -> Result<Vec<u8>, Self::Error> {
        <T as TransmuteByteConvertible>::to_bytes(data)
    }
    fn from_bytes(data: &[u8]) -> Result<Vec<Self>, Self::Error> {
        <T as TransmuteByteConvertible>::from_bytes(data)
    }
}

unsafe impl TransmuteByteConvertible for i8 {}
unsafe impl TransmuteByteConvertible for i16 {}
unsafe impl TransmuteByteConvertible for i32 {}
unsafe impl TransmuteByteConvertible for i64 {}
unsafe impl TransmuteByteConvertible for u8 {}
unsafe impl TransmuteByteConvertible for u16 {}
unsafe impl TransmuteByteConvertible for u32 {}
unsafe impl TransmuteByteConvertible for u64 {}
unsafe impl TransmuteByteConvertible for usize {}
unsafe impl TransmuteByteConvertible for isize {}
unsafe impl TransmuteByteConvertible for f32 {}
unsafe impl TransmuteByteConvertible for f64 {}
unsafe impl TransmuteByteConvertible for Complex<f32> {}
unsafe impl TransmuteByteConvertible for Complex<f64> {}

pub struct DataFrame<T: ByteConvertible> {
    pub data: Vec<T>,
}

impl<T: ByteConvertible> DataFrame<T> {
    pub fn new(data: Vec<T>) -> DataFrame<T> {
        DataFrame { data }
    }
    pub fn read(lock: &NodeGuard, port: InPortID) -> DataFrame<T> {
        lock.wait(|lock| lock.available::<usize>(port) >= 1);
        let len = lock.read_n::<usize>(port, 1).unwrap()[0];
        lock.wait(|lock| lock.available::<T>(port) >= len);
        DataFrame::new(lock.read_n::<T>(port, len).unwrap())
    }

    pub fn write(&self, lock: &NodeGuard, port: OutPortID) {
        lock.write(port, &[self.data.len()]).unwrap();
        lock.write(port, &self.data).unwrap();
    }
}

impl<T: ByteConvertible> Deref for DataFrame<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.data
    }
}

/**
 * A `NodeGuard` holds a lock on the `Graph`, which prevents other threads from interacting with
 * the graph while this object is in scope (except if execution is in a call to `wait`; see below
 * for more details).
 */
pub struct NodeGuard<'a> {
    node: &'a Node,
    graph: &'a Graph,
    guard: UnsafeCell<MutexGuard<'a, ()>>,
}

impl<'a> NodeGuard<'a> {
    fn new(graph: &'a Graph, node_id: NodeID) -> NodeGuard<'a> {
        let node = graph.node(node_id);
        let guard = UnsafeCell::new(graph.lock.lock().unwrap());
        NodeGuard { node, graph, guard }
    }

    /**
     * Block until the given function returns true. This function unlocks the graph while it waits
     * for events. The given function will be polled whenever a read or write operation occurs in
     * another node. It is given an immutable reference to this `NodeGuard` as a parameter, which
     * can be used to check availability of data.
     */
    pub fn wait<F>(&self, mut cond: F)
    where
        F: FnMut(&Self) -> bool,
    {
        while !cond(self) {
            unsafe {
                ptr::write(self.guard.get(), self.graph.cond.wait(ptr::read(self.guard.get())).unwrap());
            }
        }
    }

    /**
     * Write `data` to `port`.
     */
    pub fn write<T: ByteConvertible>(&self, port: OutPortID, data: &[T]) -> Result<(), T::Error> {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let buffer = unsafe { in_port.data() };
        let converted_data = T::to_bytes(data)?;
        buffer.extend(converted_data);
        self.graph.cond.notify_all();
        Ok(())
    }

    /**
     * Read all available data from `port`.
     */
    pub fn read<T: ByteConvertible>(&self, port: InPortID) -> Result<Vec<T>, T::Error> {
        let n = self.available::<T>(port);
        self.read_n(port, n)
    }

    /**
     * Read exactly `n` objects of type `T` from `port`. Panics if `n` objects are not available.
     */
    pub fn read_n<T: ByteConvertible>(&self, port: InPortID, n: usize) -> Result<Vec<T>, T::Error> {
        let n_bytes = n * mem::size_of::<T>();
        let in_port = self.node.in_port(port);
        let buffer = unsafe { in_port.data() };
        if buffer.len() < n_bytes {
            panic!("cannot read n! check available first!");
        }
        let out: Vec<u8> = buffer.drain(..n_bytes).collect();
        self.graph.cond.notify_all();
        T::from_bytes(&out)
    }

    /**
     * Read all available data from `port` without consuming it.
     */
    pub fn peek<T: ByteConvertible>(&self, port: InPortID) -> Result<Vec<T>, T::Error> {
        let n = self.available::<T>(port);
        self.peek_n(port, n)
    }

    /**
     * Read all available data from `port` without consuming it, starting after `index` bytes.
     *
     * Panics if there are not enough bytes available to skip to `index`.
     */
    pub fn peek_at<T: ByteConvertible>(&self, port: InPortID, index: usize) -> Result<Vec<T>, T::Error> {
        let n = self.available::<T>(port);
        assert!(n >= mem::size_of::<T>());
        self.peek_n_at(port, n - index * mem::size_of::<T>(), index)
    }

    /**
     * Read exactly `n` objects of type `T` from `port` without consuming it. Panics if `n` objects
     * of type `T` are not available starting at `index`.
     */
    pub fn peek_n<T: ByteConvertible>(&self, port: InPortID, n: usize) -> Result<Vec<T>, T::Error> {
        let n_bytes = n * mem::size_of::<T>();
        let in_port = self.node.in_port(port);
        let buffer = unsafe { in_port.data() };
        if buffer.len() < n_bytes {
            panic!("cannot read n! check available first!");
        }
        let out: Vec<u8> = buffer.iter().cloned().take(n_bytes).collect();
        self.graph.cond.notify_all();
        T::from_bytes(&out)
    }

    /**
     * Read exactly `n` objects of type `T` from `port` without consuming it, starting after
     * `index` bytes. Panics if `n` objects of type `T` are not available starting at `index`.
     */
    pub fn peek_n_at<T: ByteConvertible>(
        &self,
        port: InPortID,
        n: usize,
        index: usize,
    ) -> Result<Vec<T>, T::Error> {
        let n_bytes = n * mem::size_of::<T>();
        let in_port = self.node.in_port(port);
        let buffer = unsafe { in_port.data() };
        if buffer.len() - index < n_bytes {
            panic!("cannot read n! check available first!");
        }
        let out: Vec<u8> = buffer.iter().cloned().skip(index).take(n_bytes).collect();
        self.graph.cond.notify_all();
        T::from_bytes(&out)
    }

    /**
     * Returns the number of bytes available to be read from the given input port.
     */
    pub fn available<T: ByteConvertible>(&self, port: InPortID) -> usize {
        let in_port = self.node.in_port(port);
        let buffer = unsafe { in_port.data() };
        buffer.len() / mem::size_of::<T>()
    }

    /**
     * Returns the number of bytes buffered (i.e. waiting to be read) from the given output port.
     */
    pub fn buffered<T: ByteConvertible>(&self, port: OutPortID) -> usize {
        let edge = self.node.out_port(port).edge().unwrap();
        let endpoint_node = self.graph.node(edge.node);
        let in_port = endpoint_node.in_port(edge.port);
        let buffer = unsafe { in_port.data() };
        buffer.len() / mem::size_of::<T>()
    }
    // read_while, peek, ...

    /**
     * Gets the associated `Node`.
     */
    pub fn node(&self) -> &Node {
        self.node
    }

    /**
     * Gets the associated `Graph`.
     */
    pub fn graph(&self) -> &Graph {
        self.graph
    }
}

/**
 * Holds the context needed for a node thread to read, write, and access the graph.
 */
pub struct NodeContext {
    id: NodeID,
    graph: Arc<Graph>,
}

impl NodeContext {
    /**
     * Lock the graph, returning a `NodeGuard` which can be used for interacting with other nodes.
     */
    pub fn lock<'a>(&'a self) -> NodeGuard<'a> {
        NodeGuard::new(&*self.graph, self.id)
    }

    /**
     * Gets the associated `Node`.
     */
    pub fn node<'a>(&'a self) -> &'a Node {
        self.graph.node(self.id)
    }

    /**
     * Gets the associated `Graph`.
     */
    pub fn graph<'a>(&'a self) -> &'a Graph {
        use std::borrow::Borrow;
        self.graph.borrow()
    }
}

impl Drop for NodeContext {
    fn drop(&mut self) {
        self.graph.node(self.id).detach_thread().unwrap();
    }
}

/**
 * A `Context` takes a `Graph` and enables connecting threads to nodes.
 */
pub struct Context {
    graph: Arc<Graph>,
}

impl Context {
    /**
     * Construct a new `Context` using the given `Graph`.
     */
    pub fn new(graph: Graph) -> Context {
        Context {
            graph: Arc::new(graph),
        }
    }

    /**
     * Create the context for a specific node. This context is required for the node to interact
     * with others. The returned object can be safely moved into a thread.
     *
     * Returns `Err` if this node is already attached to another thread.
     */
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext, ()> {
        self.graph.node(node).attach_thread().map(|_| {
            NodeContext {
                graph: self.graph.clone(),
                id: node,
            }
        })
    }
}
