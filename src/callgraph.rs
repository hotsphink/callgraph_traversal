pub use petgraph::graph::NodeIndex;

use petgraph::stable_graph::StableGraph;
use petgraph::visit::IntoNodeReferences;
use regex::Regex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

pub type PropertySet = u32;

pub struct Callgraph {
    // Graph of mangled function names associated with their "limits" bit
    // vectors. NodeIndexes in this graph are also used as IDs.
    pub graph : StableGraph<String, PropertySet>,

    // Graph of the reverse relation (function callers).
    pub caller_graph : StableGraph<NodeIndex, PropertySet>,

    // Table mapping from stems (simple function names) to all functions with
    // that name.
    pub stem_table : HashMap<String, Vec<NodeIndex>>,

    // Map from IDs to all the known unmangled names of a function.
    pub alt_names : Vec<Vec<String>>,
}

pub enum DescriptionBrevity {
    Brief,
    Normal,
    Verbose,
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

pub enum Matcher<'a> {
    Substring(&'a str),
    Pattern(Regex),
}

impl<'a> Matcher<'a> {
    pub fn new(pattern : &str) -> Option<Matcher> {
        if &pattern[0..1] == "/" && &pattern[pattern.len()-1..] == "/" {
            let pattern = &pattern[1..pattern.len()-1];
            if let Ok(matcher) = Regex::new(pattern) {
                Some(Matcher::Pattern(matcher))
            } else {
                None
            }
        } else {
            Some(Matcher::Substring(pattern))
        }
    }

    pub fn is_match(&self, cg : &Callgraph, idx : NodeIndex) -> bool {
        for name in cg.names(idx) {
            match self {
                Matcher::Substring(sub) => {
                    if name.find(sub) != None {
                        return true;
                    }
                },
                Matcher::Pattern(re) => {
                    if re.is_match(name) {
                        return true;
                    }
                }
            }
        }
        return false;
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
        cg.alt_names.push(Vec::new());
        cg
    }

    pub fn add_function(&mut self, name : &str) -> NodeIndex {
        let idx = self.graph.add_node(String::from(name));
        self.caller_graph.add_node(idx);
        self.alt_names.push(Vec::new());
        idx
    }

    pub fn add_unmangled_name(&mut self, id : usize, unmangled : &str) {
        let func_stem = stem(unmangled);
        self.stem_table.entry(String::from(func_stem)).or_default().push(NodeIndex::new(id));
        self.alt_names[id].push(unmangled.to_string());
    }

    pub fn add_edge(&mut self, src : NodeIndex, dst : NodeIndex, limit : PropertySet) {
        self.graph.add_edge(src, dst, limit);
        self.caller_graph.add_edge(dst, src, limit);
    }

    pub fn names(&self, idx : NodeIndex) -> Vec<&str> {
        let mut result = Vec::<&str>::new();
        result.push(&self.graph[idx]);
        for name in &self.alt_names[idx.index()] {
            result.push(name);
        }
        return result;
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
        if pattern.len() == 0 {
            return None;
        }

        // Look for exact match with stem.
        if let Some(matches) = self.stem_table.get(pattern) {
            return Some(matches.to_vec());
        }

        // Regex match if pattern is /.../
        let mut results = Vec::<NodeIndex>::new();
        if &pattern[0..1] == "/" && &pattern[pattern.len()-1..] == "/" {
            let pattern = &pattern[1..pattern.len()-1];
            if let Ok(matcher) = Regex::new(pattern) {
                for (idx, mangled) in self.graph.node_references() {
                    if idx.index() == 0 { continue };
                    if matcher.is_match(mangled) {
                        results.push(idx);
                    } else {
                        for unmangled in &self.alt_names[idx.index()] {
                            if matcher.is_match(unmangled) {
                                results.push(idx);
                                break;
                            }
                        }
                    }
                }
            } else {
                println!("invalid regex: /{}/", pattern);
                return None
            }
            return Some(results);
        }

        // #id match
        if &pattern[0..1] == "#" {
            return match &pattern[1..].parse::<usize>() {
                Ok(n) => {
                    if *n < self.graph.node_count() {
                        Some(vec!(NodeIndex::new(*n)))
                    } else {
                        None
                    }

                },
                Err(_) => None
            };
        }

        // Exact match against mangled name.
        for (idx, func) in self.graph.node_references() {
            if pattern == func {
                results.push(idx);
            }
        }

        // Substring match against unmangled names
        if results.is_empty() {
            for (idx, names) in self.alt_names.iter().enumerate() {
                for name in names {
                    if name.find(pattern) != None {
                        results.push(NodeIndex::new(idx));
                        break
                    }
                }
            }
        }

        if ! results.is_empty() {
            return Some(results);
        }
        None
    }

    pub fn callees(&self, idx : NodeIndex) -> Vec<NodeIndex> {
        self.graph.neighbors(idx).collect()
    }

    pub fn callers(&self, idx : NodeIndex) -> Vec<NodeIndex> {
        self.caller_graph.neighbors(idx).collect()
    }

    pub fn any_route(&self, origin : NodeIndex, goal : HashSet<NodeIndex>, avoid : HashSet<NodeIndex>) -> Option<Vec<NodeIndex>> {
        let mut edges = HashMap::new();
        let mut work = VecDeque::new();
        work.push_back(origin);

        if goal.contains(&origin) {
            return Some(vec![origin]);
        }

        let mut found : Option<NodeIndex> = None;
        'search: while ! work.is_empty() {
            let src = work.pop_front().unwrap();
            for dst in self.graph.neighbors(src) {
                if edges.contains_key(&dst) { continue; }
                if avoid.contains(&dst) { continue; }
                edges.insert(dst, src);
                if goal.contains(&dst) {
                    found = Some(dst);
                    break 'search;
                }
                work.push_back(dst);
            }
        }

        if found == None {
            return None;
        }

        let mut result = vec![found.unwrap()];
        while result.last().unwrap() != &origin {
            let idx = *result.last().unwrap();
            result.push(edges[&idx]);
        }
        result.reverse();

        Some(result.to_vec())
    }
}
