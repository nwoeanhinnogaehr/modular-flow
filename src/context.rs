use super::graph::*;
use std::sync::Arc;
use std::slice;
use std::mem;
use num_complex::Complex;
use std::borrow::Borrow;
use std::thread;
use std::boxed::FnBox;

/**
 * An array of any type which is `ByteConvertible` can be converted to an array of bytes and back
 * again.
 */
pub trait ByteConvertible: Sized + Copy {
    /// Converts a slice of this into a vector of bytes, returning `None` on failure.
    fn to_bytes(data: &[Self]) -> Result<Vec<u8>>;
    /// Converts a slice of bytes into a vector of this, returning `None` on failure.
    fn from_bytes(data: &[u8]) -> Result<Vec<Self>>;
}

/**
 * Any type which is `TransmuteByteConvertible` can be converted to an array of bytes by simply
 * transmuting the underlying data.
 */
pub unsafe trait TransmuteByteConvertible: Sized + Clone {
    /**
     * Always returns `Ok`.
     */
    fn to_bytes(data: &[Self]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(unsafe {
            slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() * mem::size_of::<Self>())
        });
        Ok(out)
    }
    /**
     * Returns `Err` if the number of bytes given is not a multiple of the data size.
     */
    fn from_bytes(data: &[u8]) -> Result<Vec<Self>> {
        let mut out = Vec::new();
        out.extend_from_slice(if data.len() % mem::size_of::<Self>() == 0 {
            unsafe {
                slice::from_raw_parts(mem::transmute(data.as_ptr()), data.len() / mem::size_of::<Self>())
            }
        } else {
            return Err(Error::Conversion);
        });
        Ok(out)
    }
}

impl<T: TransmuteByteConvertible + Copy> ByteConvertible for T {
    fn to_bytes(data: &[Self]) -> Result<Vec<u8>> {
        <T as TransmuteByteConvertible>::to_bytes(data)
    }
    fn from_bytes(data: &[u8]) -> Result<Vec<Self>> {
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
unsafe impl<A, B> TransmuteByteConvertible for (A, B)
where
    A: TransmuteByteConvertible,
    B: TransmuteByteConvertible,
{
}
unsafe impl<A, B, C> TransmuteByteConvertible for (A, B, C)
where
    A: TransmuteByteConvertible,
    B: TransmuteByteConvertible,
    C: TransmuteByteConvertible,
{
}

/**
 * A `NodeGuard` holds a lock on the `Graph`, which prevents other threads from interacting with
 * the graph while this object is in scope (except if execution is in a call to `wait`; see below
 * for more details).
 */
pub struct NodeGuard<'a> {
    node: Arc<Node>,
    graph: &'a Graph,
}

impl<'a> NodeGuard<'a> {
    fn new(
        graph: &'a Graph,
        node: Arc<Node>,
        in_ports: &[Arc<InPort>],
        out_ports: &[Arc<OutPort>],
    ) -> NodeGuard<'a> {
        NodeGuard { node, graph }
    }

    /**
     * Block until the state of the locked data changes in some way.
     */
    pub fn sleep(&self) {
        let lock = self.node.lock.lock().unwrap();
        let _ = self.node.cond.wait(lock).unwrap();
    }

    /**
     * Block until the given function returns true. This function unlocks the graph while it waits
     * for events. The given function will be polled whenever a read or write operation occurs in
     * another node. It is given an immutable reference to this `NodeGuard` as a parameter, which
     * can be used to check availability of data.
     */
    pub fn wait<F>(&self, mut cond: F) -> Result<()>
    where
        F: FnMut(&Self) -> Result<bool>,
    {
        let mut lock = self.node.lock.lock().unwrap();
        loop {
            if self.node.aborting() {
                drop(lock);
                self.node.set_aborting(false, self.graph());
                return Err(Error::Aborted);
            }
            // TODO:
            // if something in cond tries to notify or wait, we will deadlock.
            // can probably provide a message in this case by careful tracking of threads
            if cond(self)? {
                break;
            }
            lock = self.node.cond.wait(lock).unwrap();
        }
        Ok(())
    }

    /**
     * Write `data` to `port`.
     */
    pub fn write<T: ByteConvertible>(&self, port: OutPortID, data: &[T]) -> Result<()> {
        let node = self.node();
        let out_port = node.out_port(port)?;
        let edge = match out_port.edge() {
            Some(e) => e,
            None => return Err(Error::NotConnected),
        };
        let endpoint_node = self.graph.node(edge.node)?;
        let in_port = endpoint_node.in_port(edge.port)?;
        let converted_data = T::to_bytes(data)?;
        if converted_data.len() > 0 {
            let mut buffer = in_port.details().data();
            buffer.extend(converted_data);
            drop(buffer);
            node.notify_self();
            endpoint_node.notify_self();
        }
        Ok(())
    }

    /**
     * Read all available data from `port`.
     */
    pub fn read<T: ByteConvertible>(&self, port: InPortID) -> Result<Vec<T>> {
        let n = self.available::<T>(port)?;
        self.read_n(port, n)
    }

    /**
     * Read exactly `n` objects of type `T` from `port`.
     */
    pub fn read_n<T: ByteConvertible>(&self, port: InPortID, n: usize) -> Result<Vec<T>> {
        let node = self.node();
        let n_bytes = n * mem::size_of::<T>();
        let in_port = node.in_port(port)?;
        let edge = match in_port.edge() {
            Some(e) => e,
            None => return Err(Error::NotConnected),
        };
        let endpoint_node = self.graph.node(edge.node)?;
        let mut buffer = in_port.details().data();
        if buffer.len() < n_bytes {
            return Err(Error::Unavailable);
        }
        let out: Vec<u8> = buffer.drain(..n_bytes).collect();
        drop(buffer);
        if out.len() > 0 {
            node.notify_self();
            endpoint_node.notify_self();
        }
        T::from_bytes(&out)
    }

    /**
     * Read all available data from `port` without consuming it.
     */
    pub fn peek<T: ByteConvertible>(&self, port: InPortID) -> Result<Vec<T>> {
        self.peek_at(port, 0)
    }

    /**
     * Read all available data from `port` without consuming it, starting after `index` bytes.
     */
    pub fn peek_at<T: ByteConvertible>(&self, port: InPortID, index: usize) -> Result<Vec<T>> {
        let n = self.available::<T>(port)?;
        if n < mem::size_of::<T>() {
            return Err(Error::Unavailable);
        }
        self.peek_n_at(port, n - index * mem::size_of::<T>(), index)
    }

    /**
     * Read exactly `n` objects of type `T` from `port` without consuming it.
     */
    pub fn peek_n<T: ByteConvertible>(&self, port: InPortID, n: usize) -> Result<Vec<T>> {
        self.peek_n_at(port, n, 0)
    }

    /**
     * Read exactly `n` objects of type `T` from `port` without consuming it, starting after
     * `index` bytes.
     */
    pub fn peek_n_at<T: ByteConvertible>(&self, port: InPortID, n: usize, index: usize) -> Result<Vec<T>> {
        let n_bytes = n * mem::size_of::<T>();
        let in_port = self.node().in_port(port)?;
        let buffer = in_port.details().data();
        if buffer.len() - index < n_bytes {
            return Err(Error::Unavailable);
        }
        let out: Vec<u8> = buffer.iter().cloned().skip(index).take(n_bytes).collect();
        T::from_bytes(&out)
    }

    /**
     * Returns the number of bytes available to be read from the given input port.
     */
    pub fn available<T: ByteConvertible>(&self, port: InPortID) -> Result<usize> {
        self.available_at::<T>(port, 0)
    }

    /**
     * Returns the number of objects of type `T` available to be read from the given input port,
     * after skipping `index` bytes.
     */
    pub fn available_at<T: ByteConvertible>(&self, port: InPortID, index: usize) -> Result<usize> {
        let in_port = self.node().in_port(port)?;
        if in_port.edge().is_none() {
            return Err(Error::NotConnected);
        }
        let buffer = in_port.details().data();
        if buffer.len() < index {
            return Err(Error::Unavailable);
        }
        Ok((buffer.len() - index) / mem::size_of::<T>())
    }

    /**
     * Returns the number of objects of type `T` buffered (i.e. waiting to be read) from the given
     * output port.
     */
    pub fn buffered<T: ByteConvertible>(&self, port: OutPortID) -> Result<usize> {
        let out_port = self.node().out_port(port)?;
        let edge = match out_port.edge() {
            Some(e) => e,
            None => return Err(Error::NotConnected),
        };
        let endpoint_node = self.graph.node(edge.node)?;
        let in_port = endpoint_node.in_port(edge.port)?;
        let buffer = in_port.details().data();
        Ok(buffer.len() / mem::size_of::<T>())
    }
    // read_while, peek, ...

    /**
     * Gets the associated `Node`.
     */
    pub fn node(&self) -> &Node {
        &*self.node
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
    node: Arc<Node>,
    graph: Arc<Graph>,
}

impl NodeContext {
    /**
     * Lock the graph, returning a `NodeGuard` which can be used for interacting with other nodes.
     */
    pub fn lock<'a>(&'a self, in_ports: &[Arc<InPort>], out_ports: &[Arc<OutPort>]) -> NodeGuard<'a> {
        NodeGuard::new(self.graph(), self.node.clone(), in_ports, out_ports)
    }

    /**
     * Lock all input and output ports.
     */
    pub fn lock_all<'a>(&'a self) -> NodeGuard<'a> {
        NodeGuard::new(self.graph(), self.node.clone(), &self.node().in_ports(), &self.node().out_ports())
    }

    /**
     * Gets the associated `Graph`.
     */
    pub fn graph<'a>(&'a self) -> &'a Graph {
        self.graph.borrow()
    }

    /**
     * Gets the associated `Node`.
     */
    pub fn node<'a>(&'a self) -> &Node {
        self.node.borrow()
    }
}

impl Drop for NodeContext {
    fn drop(&mut self) {
        self.node.detach_thread(&*self.graph).unwrap();
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
    pub fn node_ctx<'a>(&'a self, node: NodeID) -> Result<NodeContext> {
        let node = self.graph.node(node)?;
        node.attach_thread(&*self.graph).map(|_| {
            NodeContext {
                graph: self.graph.clone(),
                node: node,
            }
        })
    }

    /**
     * Gets the associated `Graph`.
     */
    pub fn graph(&self) -> &Graph {
        &*self.graph
    }
}
