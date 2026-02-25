use dep::{
    build_dependency_graph, filter_graph, graph_to_dot, graph_to_json,
    EmptyLogger, WalkBuilder, NodeKind,
};
use dep_core::test_util::TestFS;
use dep_core::{resolve_node_kind, js_resolve::JS_EXTENSIONS};
use proptest::prelude::*;

#[test]
fn test_js_parser_basic() {
    let fs = TestFS::new([("a.js", "import './b.js';"), ("b.js", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    assert!(graph.node_indices().any(|i| graph[i].name == "a.js"));
}

#[test]
fn test_js_parser_malformed() {
    let fs = TestFS::new([("a.js", "import ???")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let res = build_dependency_graph(&walk, None, &logger);
    assert!(res.is_ok());
}

#[test]
fn test_mixed_extension_imports() {
    let fs = TestFS::new([
        ("a.ts", "import './b';\nimport './c.js';"),
        ("b.ts", ""),
        ("c.js", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let a_idx = graph.node_indices().find(|i| graph[*i].name == "a.ts").unwrap();
    let b_idx = graph.node_indices().find(|i| graph[*i].name == "b.ts").unwrap();
    let c_idx = graph.node_indices().find(|i| graph[*i].name == "c.js").unwrap();
    assert!(graph.find_edge(a_idx, b_idx).is_some());
    assert!(graph.find_edge(a_idx, c_idx).is_some());
}

#[test]
fn test_asset_node_kind() {
    let fs = TestFS::new([("index.js", "import './logo.svg';"), ("logo.svg", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let js_idx = graph.node_indices().find(|i| graph[*i].name == "index.js").unwrap();
    let asset_idx = graph.node_indices().find(|i| graph[*i].name == "logo.svg").unwrap();
    assert!(graph.find_edge(js_idx, asset_idx).is_some());
}

#[test]
fn test_require_and_module_exports() {
    let fs = TestFS::new([
        ("index.js", "const foo = require('./foo');\nimport './bar.js';\nmodule.exports = foo;"),
        ("foo.js", ""),
        ("bar.js", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let main_idx = graph.node_indices().find(|i| graph[*i].name == "index.js").unwrap();
    let foo_idx = graph.node_indices().find(|i| graph[*i].name == "foo.js").unwrap();
    let bar_idx = graph.node_indices().find(|i| graph[*i].name == "bar.js").unwrap();
    assert!(graph.find_edge(main_idx, foo_idx).is_some());
    assert!(graph.find_edge(main_idx, bar_idx).is_some());
}

#[test]
fn test_other_extensions() {
    let fs = TestFS::new([("a.mjs", "import './b.cjs';"), ("b.cjs", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let a_idx = graph.node_indices().find(|i| graph[*i].name == "a.mjs").unwrap();
    let b_idx = graph.node_indices().find(|i| graph[*i].name == "b.cjs").unwrap();
    assert!(graph.find_edge(a_idx, b_idx).is_some());
}

#[test]
fn test_tsx_parsing_with_jsx() {
    let fs = TestFS::new([
        ("a.tsx", "import './b';\nexport const App = () => <div>Hello</div>;"),
        ("b.ts", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let a_idx = graph.node_indices().find(|i| graph[*i].name == "a.tsx").unwrap();
    let b_idx = graph.node_indices().find(|i| graph[*i].name == "b.ts").unwrap();
    assert!(graph.find_edge(a_idx, b_idx).is_some(), "Edge from a.tsx to b.ts missing");
}

#[test]
fn test_jsx_parsing() {
    let fs = TestFS::new([
        ("a.jsx", "import './b';\nexport const App = () => <div>Hello</div>;"),
        ("b.js", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let a_idx = graph.node_indices().find(|i| graph[*i].name == "a.jsx").unwrap();
    let b_idx = graph.node_indices().find(|i| graph[*i].name == "b.js").unwrap();
    assert!(graph.find_edge(a_idx, b_idx).is_some(), "Edge from a.jsx to b.js missing");
}

#[test]
fn test_html_parser_basic() {
    let fs = TestFS::new([
        ("index.html", "<script src=\"./app.js\"></script>"),
        ("app.js", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let html_idx = graph.node_indices().find(|i| graph[*i].name == "index.html").unwrap();
    let js_idx = graph.node_indices().find(|i| graph[*i].name == "app.js").unwrap();
    assert!(graph.find_edge(html_idx, js_idx).is_some());
}

#[test]
fn test_html_parser_malformed() {
    let fs = TestFS::new([
        ("index.html", "<script src='broken.js'>"),
        ("broken.js", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let res = build_dependency_graph(&walk, None, &logger);
    assert!(res.is_ok());
}

#[test]
fn test_mdx_parser_basic() {
    let fs = TestFS::new([
        ("index.mdx", "import Foo from './foo.js'\n\n# Hello"),
        ("foo.js", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let mdx_idx = graph.node_indices().find(|i| graph[*i].name == "index.mdx").unwrap();
    let foo_idx = graph.node_indices().find(|i| graph[*i].name == "foo.js").unwrap();
    assert!(graph.find_edge(mdx_idx, foo_idx).is_some());
}

#[test]
fn test_vite_glob_basic() {
    let fs = TestFS::new([
        ("index.ts", "const modules = import.meta.glob('./foo/*.jsx', { eager: true }) as any;"),
        ("foo/a.jsx", ""),
        ("foo/b.jsx", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let idx_index = graph.node_indices().find(|i| graph[*i].name == "index.ts").unwrap();
    let idx_a = graph.node_indices().find(|i| graph[*i].name == "foo/a.jsx").unwrap();
    let idx_b = graph.node_indices().find(|i| graph[*i].name == "foo/b.jsx").unwrap();
    assert!(graph.find_edge(idx_index, idx_a).is_some());
    assert!(graph.find_edge(idx_index, idx_b).is_some());
}

#[test]
fn test_vite_glob_asset() {
    let fs = TestFS::new([
        ("index.js", "const imgs = import.meta.glob('./assets/*.png', { eager: true }) as any;"),
        ("assets/logo.png", ""),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let idx_index = graph.node_indices().find(|i| graph[*i].name == "index.js").unwrap();
    let idx_logo = graph.node_indices().find(|i| graph[*i].name == "assets/logo.png").unwrap();
    assert!(graph.find_edge(idx_index, idx_logo).is_some());
}

#[test]
fn test_package_parsers_basic() {
    let fs = TestFS::new([
        ("pkg/package.json", b"{\"name\":\"pkg\",\"main\":\"index.js\"}" as &[u8]),
        ("pkg/index.js", b"" as &[u8]),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    assert!(graph.node_indices().any(|i| graph[i].name == "pkg"));
}

#[test]
fn test_package_parsers_malformed() {
    let fs = TestFS::new([("pkg/package.json", "not json")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let res = build_dependency_graph(&walk, None, &logger);
    assert!(res.is_ok());
}

#[test]
fn test_malformed_package_json_is_ignored() {
    let fs = TestFS::new([("pkg/package.json", "notjson")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let res = build_dependency_graph(&walk, None, &logger);
    assert!(res.is_ok());
}

#[test]
fn test_package_nodes_and_edges() {
    let fs = TestFS::new([
        (
            "packages/a/package.json",
            b"{\"name\":\"a\",\"main\":\"index.js\",\"dependencies\":{\"b\":\"workspace:*\",\"ext\":\"1\"}}" as &[u8]
        ),
        ("packages/a/index.js", b"" as &[u8]),
        ("packages/b/package.json", b"{\"name\":\"b\"}" as &[u8]),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let a_idx = graph.node_indices().find(|i| graph[*i].name == "a").unwrap();
    let b_idx = graph.node_indices().find(|i| graph[*i].name == "b").unwrap();
    let main_idx = graph.node_indices().find(|i| graph[*i].name == "packages/a/index.js").unwrap();
    assert!(graph.find_edge(a_idx, b_idx).is_some());
    assert!(graph.find_edge(a_idx, main_idx).is_some());
    assert!(graph.node_indices().any(|i| graph[i].name == "ext"));
}

#[test]
fn test_folder_nodes() {
    let fs = TestFS::new([("foo/bar.js", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let folder_idx = graph.node_indices().find(|i| graph[*i].name == "foo").unwrap();
    let file_idx = graph.node_indices().find(|i| graph[*i].name == "foo/bar.js").unwrap();
    assert!(graph.find_edge(folder_idx, file_idx).is_some());
    assert_eq!(resolve_node_kind(&graph, folder_idx), NodeKind::Folder);

    let without = graph_to_dot(&filter_graph(&graph, true, true, false, true, true, &[]));
    assert!(without.contains("foo/bar.js"));
    assert!(!without.contains("shape=folder"));

    let with = graph_to_dot(&filter_graph(&graph, true, true, true, true, true, &[]));
    assert!(with.contains("shape=folder"));
}

#[test]
fn test_asset_filter() {
    let fs = TestFS::new([("index.js", "import './style.css';"), ("style.css", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let js_idx = graph.node_indices().find(|i| graph[*i].name == "index.js").unwrap();
    let css_idx = graph.node_indices().find(|i| graph[*i].name == "style.css").unwrap();
    assert!(graph.find_edge(js_idx, css_idx).is_some());
    assert_eq!(resolve_node_kind(&graph, css_idx), NodeKind::Asset);

    let without = graph_to_dot(&filter_graph(&graph, true, true, false, false, true, &[]));
    assert!(!without.contains("style.css"));
    let with = graph_to_dot(&filter_graph(&graph, true, true, false, true, true, &[]));
    assert!(with.contains("style.css"));
}

#[test]
fn test_json_output() {
    let fs = TestFS::new([("index.js", "import './b.js';"), ("b.js", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let json = graph_to_json(&filter_graph(&graph, true, true, false, true, true, &[]));
    assert!(json.contains("index.js"));
    assert!(json.contains("b.js"));
}

#[test]
fn test_ignore_nodes() {
    let fs = TestFS::new([("a.js", ""), ("b.js", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let dot = graph_to_dot(&filter_graph(
        &graph, true, true, false, true, true,
        &["b.js".to_string()],
    ));
    assert!(dot.contains("a.js"));
    assert!(!dot.contains("b.js"));
}

#[test]
fn test_tsconfig_paths() {
    let fs = TestFS::new([
        ("tsconfig.json", b"{\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@foo/*\": [\"foo/*\"] }\n  }\n}" as &[u8]),
        ("index.ts", b"import '@foo/bar';" as &[u8]),
        ("foo/bar.ts", b"" as &[u8]),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let idx_index = graph.node_indices().find(|i| graph[*i].name == "index.ts").unwrap();
    let idx_target = graph.node_indices().find(|i| graph[*i].name == "foo/bar.ts").unwrap();
    assert!(graph.find_edge(idx_index, idx_target).is_some());
}

#[test]
fn test_tsconfig_jsonc_with_comments() {
    let fs = TestFS::new([
        ("tsconfig.json", b"{\n  // comment\n  \"compilerOptions\": {\n    /* base */ \"baseUrl\": \".\",\n    \"paths\": { \"@foo/*\": [\"foo/*\",] }\n  }\n}" as &[u8]),
        ("index.ts", b"import '@foo/bar';" as &[u8]),
        ("foo/bar.ts", b"" as &[u8]),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let idx_index = graph.node_indices().find(|i| graph[*i].name == "index.ts").unwrap();
    let idx_target = graph.node_indices().find(|i| graph[*i].name == "foo/bar.ts").unwrap();
    assert!(graph.find_edge(idx_index, idx_target).is_some());
}

#[test]
fn test_alias_and_relative_refer_to_same_file() {
    let fs = TestFS::new([
        ("tsconfig.json", b"{\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@lib/*\": [\"lib/*\"] }\n  }\n}" as &[u8]),
        ("a.ts", b"import './lib/c';" as &[u8]),
        ("b.ts", b"import '@lib/c';" as &[u8]),
        ("lib/c.ts", b"" as &[u8]),
    ]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let idx_a = graph.node_indices().find(|i| graph[*i].name == "a.ts").unwrap();
    let idx_b = graph.node_indices().find(|i| graph[*i].name == "b.ts").unwrap();
    let idx_c = graph.node_indices().find(|i| graph[*i].name == "lib/c.ts").unwrap();
    assert!(graph.find_edge(idx_a, idx_c).is_some());
    assert!(graph.find_edge(idx_b, idx_c).is_some());
}

#[test]
fn test_malformed_tsconfig_does_not_fail() {
    let fs = TestFS::new([("tsconfig.json", "not json"), ("index.ts", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let res = build_dependency_graph(&walk, None, &logger);
    assert!(res.is_ok());
}

#[test]
fn test_json_output_from_json_mod() {
    let fs = TestFS::new([("index.js", "import './b.js';"), ("b.js", "")]);
    let root = fs.root();
    let logger = EmptyLogger;
    let walk = WalkBuilder::new(&root).build();
    let graph = build_dependency_graph(&walk, None, &logger).unwrap();
    let json = graph_to_json(&filter_graph(&graph, true, true, false, true, true, &[]));
    assert!(json.contains("index.js"));
    assert!(json.contains("b.js"));
}

proptest! {
    #[test]
    fn prop_resolve_relative_import_find(ext in proptest::sample::select(JS_EXTENSIONS)) {
        let fs = TestFS::new([(format!("dir/foo.{}", ext), "")]);
        let root = fs.root();
        let dir = root.join("dir").unwrap();
        prop_assert!(dep_core::js_resolve::resolve_relative_import(&dir, "./foo").is_some());
    }
}
