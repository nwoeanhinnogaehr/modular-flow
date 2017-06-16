/// graph
pub mod graph;

#[cfg(test)]
mod tests {
    use graph::*;

    #[test]
    fn graph_connect() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        let sink = g.add_node(1, 0);
        g.disconnect(sink, 0).unwrap_err();
        g.connect(source, 0, sink, 0).unwrap();
        g.connect(source, 0, sink, 0).unwrap_err();
        g.disconnect(sink, 0).unwrap();
        g.disconnect(sink, 0).unwrap_err();
        g.connect(source, 0, sink, 0).unwrap();
    }

    #[test]
    fn node_types() {
        let mut g = Graph::new();
        let source = g.add_node(0, 1);
        assert_eq!(g.get_node(source).node_type(), NodeType::Source);
        let sink = g.add_node(6, 0);
        assert_eq!(g.get_node(sink).node_type(), NodeType::Sink);
        let internal = g.add_node(1, 22);
        assert_eq!(g.get_node(internal).node_type(), NodeType::Internal);
        let useless = g.add_node(0, 0);
        assert_eq!(g.get_node(useless).node_type(), NodeType::Useless);
    }
}
