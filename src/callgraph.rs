extern crate petgraph;

pub use petgraph::graph::NodeIndex;

use petgraph::stable_graph::{StableGraph, Neighbors};
use petgraph::visit::IntoNodeReferences;
use regex::Regex;
use std::collections::HashMap;
use std::collections::VecDeque;

pub struct Callgraph {
    // Graph of mangled function names associated with their "limits" bit
    // vectors. NodeIndexes in this graph are also used as IDs.
    pub graph : StableGraph<String, u32>,

    // Graph of the reverse relation (function callers).
    pub caller_graph : StableGraph<NodeIndex, u32>,

    // Table mapping from stems (simple function names) to all functions with
    // that name.
    pub stem_table : HashMap<String, Vec<NodeIndex>>,

    // Map from IDs to all the known unmangled names of a function.
    pub alt_names : Vec<Vec<String>>,
}

pub enum DescriptionBrevity {
    Brief,
    Normal,
    Verbose
}

lazy_static! {
    static ref STEM_RE : Regex = Regex::new(r"([\w_]+)\(").unwrap();
}

fn stem(raw : &str) -> &str {
    match STEM_RE.captures(raw) {
        Some(m) => m.get(1).unwrap().as_str(),
        None => raw
    }
}

impl Callgraph {
    pub fn new() -> Callgraph {
        let mut cg = Callgraph {
            graph: StableGraph::new(),
            caller_graph: StableGraph::new(),
            stem_table: HashMap::new(),
            alt_names: Vec::new(),
        };
        let idx = cg.graph.add_node(String::from("(dummy node zero)"));
        cg.caller_graph.add_node(idx);
        cg
    }

    pub fn add_function(&mut self, name : &str) -> NodeIndex {
        let idx = self.graph.add_node(String::from(name));
        self.caller_graph.add_node(idx);
        idx
    }

    pub fn add_unmangled_name(&mut self, id : usize, unmangled : &str) {
        let func_stem = stem(unmangled);
        self.stem_table.entry(String::from(func_stem)).or_default().push(NodeIndex::new(id));
        // TODO: Add to the list of names associated with an id.
        if id >= self.alt_names.len() {
            self.alt_names.resize(id + 1, Vec::new());
        }
        self.alt_names[id].push(unmangled.to_string());
    }

    pub fn add_edge(&mut self, src : NodeIndex, dst : NodeIndex, limit : u32) {
        self.graph.add_edge(src, dst, limit);
        self.caller_graph.add_edge(dst, src, limit);
    }

    pub fn name(&self, idx : NodeIndex, brevity : DescriptionBrevity) -> String {
        match brevity {
            DescriptionBrevity::Brief => self.graph[idx].to_string(),

            DescriptionBrevity::Normal => {
                let alt = &self.alt_names[idx.index()];
                if alt.is_empty() {
                    format!("#{} = {}", idx.index(), self.graph[idx])
                } else {
                    format!("#{} = {}", idx.index(), alt[0])
                }
            },

            DescriptionBrevity::Verbose => {
                let mut s = format!("#{} = {}", idx.index(), self.graph[idx]);
                for unmangled in &self.alt_names[idx.index()] {
                    s += &("\n  ".to_owned() + &unmangled);
                }
                s
            },
        }
    }

    pub fn resolve(&self, pattern : &str) -> Option<Vec<NodeIndex>> {
        if let Some(matches) = self.stem_table.get(pattern) {
            return Some(matches.to_vec());
        }
        let mut results = Vec::<NodeIndex>::new();
        if &pattern[0..1] == "/" && &pattern[pattern.len()-1..] == "/" {
            let matcher = Regex::new(&pattern[1..pattern.len()-2]).unwrap();
            for (idx, func) in self.graph.node_references() {
                if matcher.is_match(func) && idx.index() != 0 {
                    results.push(idx);
                }
            }
            return Some(results);
        }
        if &pattern[0..1] == "#" {
            return match &pattern[1..].parse::<usize>() {
                Ok(n) => Some(vec!(NodeIndex::new(*n))),
                Err(_) => None
            };
        }
        for (idx, func) in self.graph.node_references() {
            if func.find(pattern) != None {
                results.push(idx);
            }
        }
        if ! results.is_empty() {
            return Some(results);
        }
        None
    }

    pub fn callees(&self, idx : NodeIndex) -> Neighbors<u32> {
        self.graph.neighbors(idx)
    }

    pub fn callers(&self, idx : NodeIndex) -> Vec<NodeIndex> {
        self.caller_graph.neighbors(idx).collect()
    }

    pub fn any_route(&self, origin : NodeIndex, goal : NodeIndex, _avoid : Vec<NodeIndex>) -> Option<Vec<NodeIndex>> {
        let mut edges : HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut work : VecDeque<NodeIndex> = VecDeque::new();
        work.push_back(origin);

        if origin == goal {
            return Some(vec![origin]);
        }

        while ! work.is_empty() {
            let src = work.pop_front().unwrap();
            for dst in self.graph.neighbors(src) {
                if edges.contains_key(&dst) { continue; }
                edges.insert(dst, src);
                if dst == goal {
                    break;
                }
                work.push_back(dst);
            }
        }

        if ! edges.contains_key(&goal) {
            return None;
        }

        let mut result = vec![goal];
        while result.last().unwrap() != &origin {
            let idx = *result.last().unwrap();
            //result.push(*edges.get(&idx).unwrap());
            result.push(edges[&idx]);
        }
        result.reverse();

        Some(result.to_vec())
    }
}
