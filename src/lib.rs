
/// graph
pub mod graph;
pub mod supervisor;

#[cfg(test)]
mod tests {
    use graph::*;
    use supervisor::*;
    use std::thread;

    #[test]
    fn graph_connect() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        let sink = g.add_node(1, 0);
        g.disconnect(sink, InPortID(0)).unwrap_err();
        g.connect(source, OutPortID(0), sink, InPortID(0)).unwrap();
        g.connect(source, OutPortID(0), sink, InPortID(0))
            .unwrap_err();
        g.disconnect(sink, InPortID(0)).unwrap();
        g.disconnect(sink, InPortID(0)).unwrap_err();
        g.connect(source, OutPortID(0), sink, InPortID(0)).unwrap();
    }

    #[test]
    fn graph_connect_many() {
        let mut g = Graph::new();
        let source = g.add_node(0, 2);
        let sink = g.add_node(2, 0);
        g.disconnect(sink, InPortID(0)).unwrap_err();
        g.disconnect(sink, InPortID(1)).unwrap_err();
        g.connect(source, OutPortID(0), sink, InPortID(0)).unwrap();
        g.connect(source, OutPortID(0), sink, InPortID(0))
            .unwrap_err();
        g.connect(source, OutPortID(1), sink, InPortID(0))
            .unwrap_err();
        g.connect(source, OutPortID(1), sink, InPortID(1)).unwrap();
        g.disconnect(sink, InPortID(0)).unwrap();
        g.disconnect(sink, InPortID(0)).unwrap_err();
        g.connect(source, OutPortID(0), sink, InPortID(1))
            .unwrap_err();
        g.disconnect(sink, InPortID(1)).unwrap();
        g.connect(source, OutPortID(0), sink, InPortID(1)).unwrap();
    }

    #[test]
    fn node_types() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        assert_eq!(g.node(source).node_type(), NodeType::Source);
        let sink = g.add_node(6, 0);
        assert_eq!(g.node(sink).node_type(), NodeType::Sink);
        let internal = g.add_node(1, 22);
        assert_eq!(g.node(internal).node_type(), NodeType::Internal);
        let useless = g.add_node(0, 0);
        assert_eq!(g.node(useless).node_type(), NodeType::Observer);
    }
    #[test]
    fn supervisor_test() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        let internal = g.add_node(1, 1);
        let sink = g.add_node(1, 0);
        g.connect(source, OutPortID(0), internal, InPortID(0))
            .unwrap();
        g.connect(internal, OutPortID(0), sink, InPortID(0))
            .unwrap();
        let s = Supervisor::new(g);
        let src_ctx = s.node_ctx(source).unwrap();
        thread::spawn(move || loop {
            let data: Vec<u8> = vec![1, 2, 3, 4, 5];
            src_ctx.write(OutPortID(0), &data);
        });
        let int_ctx = s.node_ctx(internal).unwrap();
        thread::spawn(move || loop {
            let data = int_ctx.read_n::<u8>(InPortID(0), 1);
            println!("int {:?}", &*data);
            int_ctx.write(OutPortID(0), &*data);
            int_ctx.write(OutPortID(0), &*data);
        });
        let snk_ctx = s.node_ctx(sink).unwrap();
        thread::spawn(move || loop {
            let data = snk_ctx.read_n::<u8>(InPortID(0), 3);
            println!("sink {:?}", &*data);
        });
        s.run();
    }
}
