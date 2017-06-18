/// graph
pub mod graph;
pub mod supervisor;

#[cfg(test)]
mod tests {
    use graph::*;
    use supervisor::*;

    #[test]
    fn graph_connect() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        let sink = g.add_node(1, 0);
        g.disconnect(sink, InPortID(0)).unwrap_err();
        g.connect(source, OutPortID(0), sink, InPortID(0)).unwrap();
        g.connect(source, OutPortID(0), sink, InPortID(0)).unwrap_err();
        g.disconnect(sink, InPortID(0)).unwrap();
        g.disconnect(sink, InPortID(0)).unwrap_err();
        g.connect(source, OutPortID(0), sink, InPortID(0)).unwrap();
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
        assert_eq!(g.node(useless).node_type(), NodeType::Useless);
    }
    #[test]
    fn supervisor_test() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        let internal = g.add_node(1, 1);
        let sink = g.add_node(1, 0);
        g.connect(source, OutPortID(0), internal, InPortID(0)).unwrap();
        g.connect(internal, OutPortID(0), sink, InPortID(0)).unwrap();
        let mut s = Supervisor::new(g);
        s.run();
    }
}
