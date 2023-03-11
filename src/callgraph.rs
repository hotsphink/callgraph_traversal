pub use petgraph::graph::{
    Graph,
    NodeIndex,
    EdgeIndex,
    EdgeReference
};

use petgraph::visit::{EdgeRef, IntoNodeReferences};
use regex::Regex;
use std::collections::{
    HashMap,
    HashSet,
    VecDeque
};

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Copy, Clone, Debug)]
pub struct PropertySet {
    pub all : u32,
    pub any : u32
}

pub struct Callgraph {
    // Graph of mangled function names associated with their "limits" bit
    // vectors. NodeIndexes in this graph are also used as IDs.
    pub graph : Graph<String, PropertySet>,

    pub roots : Option<HashSet<NodeIndex>>,
    pub sinks : Option<HashSet<NodeIndex>>,

    root : NodeIndex,
    sink : NodeIndex,

    // Graph of the reverse relation (function callers).
    pub caller_graph : Graph<NodeIndex, PropertySet>,

    // Table mapping from stems (simple function names) to all functions with
    // that name.
    pub stem_table : HashMap<String, Vec<NodeIndex>>,

    // Map from IDs to all the known unmangled names of a function.
    pub alt_names : Vec<Vec<String>>,

    // Bits to descriptions of properties.
    pub property_names : HashMap<u32, String>,
}

pub enum DescriptionBrevity {
    _Brief,
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
        false
    }
}

impl Callgraph {
    pub fn new() -> Callgraph {
        let mut cg = Callgraph {
            graph: Graph::new(),
	    roots: None,
	    sinks: None,
            root: NodeIndex::new(0),
            sink: NodeIndex::new(0),
            caller_graph: Graph::new(),
            stem_table: HashMap::new(),
            alt_names: Vec::new(),
            property_names: HashMap::new(),
        };
        let idx = cg.graph.add_node(String::from("(dummy node zero)"));
        cg.caller_graph.add_node(idx);
        cg.alt_names.push(Vec::new());
        cg.property_names.insert(1, "GC_SUPPRESSED".to_string());
        cg.property_names.insert(2, "CANSCRIPT_BOUNDED".to_string());
        cg.property_names.insert(4, "DOM_ITERATING".to_string());
        cg.property_names.insert(8, "NONRELEASING".to_string());
        cg.property_names.insert(16, "REPLACED".to_string());
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
        result
    }

    pub fn name(&self, idx : NodeIndex, brevity : DescriptionBrevity) -> String {
        match brevity {
            DescriptionBrevity::_Brief => self.graph[idx].to_string(),

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
                    s += &("\n  ".to_owned() + unmangled);
                }
                s
            },
        }
    }

    pub fn describe_property_set(&self, propset : u32) -> String {
        // Someday I'll find somebody to teach me Rust.
        let mut s = self.property_names.iter().map(
            |(bit, desc)| if (propset & bit) != 0 { Some(desc) } else { None }
        ).into_iter().flatten().fold(String::new(), |mut a, b| {
            a.reserve(b.len() + 1);
            a.push_str(b);
            a.push_str(",");
            a
        }).to_string();
        s.pop();
        s
    }

    pub fn resolve_property(&self, query : &str) -> Option<u32> {
        for (prop, name) in self.property_names.iter() {
            if name == query {
                return Some(*prop)
            }
        }
        None
    }
    
    pub fn describe_edge(&self, idx : EdgeIndex, brevity : DescriptionBrevity) -> String {
        let target = self.graph.edge_endpoints(idx).unwrap().1;
        let node_str = self.name(target, brevity);
        let (any, all) = (self.graph[idx].any, self.graph[idx].all);
        match any {
            0 => node_str,
            x if x == all => node_str + " [" + &self.describe_property_set(any) + "]",
            _ => node_str + " [" + &self.describe_property_set(any) + ":" + &self.describe_property_set(all) + "]",
        }
    }

    pub fn resolve(&self, pattern : &str) -> Option<Vec<NodeIndex>> {
        if pattern.is_empty() {
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
        self.graph.neighbors(idx).filter(|n| *n != self.sink).collect()
    }

    pub fn callee_edges(&self, idx : NodeIndex) -> Vec<EdgeIndex> {
        self.graph.edges(idx).
            filter(|e| e.target() != self.sink).
            map(|e| e.id()).
            collect()
    }

    pub fn callers(&self, idx : NodeIndex) -> Vec<NodeIndex> {
        self.caller_graph.neighbors(idx).filter(|n| *n != self.root).collect()
    }

    pub fn caller_edges(&self, idx : NodeIndex) -> Vec<EdgeIndex> {
        self.caller_graph.edges(idx).
            filter(|e| e.target() != self.root).
            map(|e| e.id()).
            collect()
    }

    // FIXME: If there are many origins (eg AddRef), then this could do a large
    // traversal N times. Sample: `route from AddRef to (GC) avoiding #2`.
    pub fn any_route_from_one_of(
        &self,
        origins : &[NodeIndex],
        goal : &HashSet<NodeIndex>,
        avoid : &HashSet<NodeIndex>,
        avoid_props : u32
    ) -> Option<Vec<EdgeIndex>>
    {
        let mut bestpath : Option<Vec<EdgeIndex>> = None;
        for origin in origins {
            if avoid.contains(origin) { continue; }
            if let Some(path) = self.any_route(*origin, goal, avoid, avoid_props) {
                if let Some(prev) = bestpath.as_ref() {
                    if prev.len() > path.len() {
                        bestpath = Some(path);
                    }
                } else {
                    bestpath = Some(path);
                }
            }
        }

        bestpath
    }

    pub fn any_route(
        &self,
        origin : NodeIndex,
        goal : &HashSet<NodeIndex>,
        avoid : &HashSet<NodeIndex>,
        avoid_props : u32
    ) -> Option<Vec<EdgeIndex>>
    {
        // Map from node to the edge that led to that node.
        let mut edges : HashMap<NodeIndex, EdgeReference<PropertySet>> = HashMap::new();
        let mut work = VecDeque::new();
        work.push_back(origin);

        let mut found : Option<EdgeReference<PropertySet>> = None;
        'search: while ! work.is_empty() {
            let src = work.pop_front().unwrap();
            for edge in self.graph.edges(src) {
                let dst = edge.target();
                if edges.contains_key(&dst) { continue; }
                if avoid.contains(&dst) { continue; }
                if (avoid_props & self.graph[edge.id()].all) != 0 { continue; }
                edges.insert(dst, edge);
                if goal.contains(&dst) {
                    found = Some(edge);
                    break 'search;
                }
                work.push_back(dst);
            }
        }

        if found == None {
            return None;
        }

        let mut path = vec![found.unwrap()];
        while path.last().unwrap().source() != origin {
            let next = path.last().unwrap();
            path.push(edges[&next.source()]);
        }

        let mut result : Vec<EdgeIndex> = vec![];
        while let Some(edge) = path.pop() {
            result.push(edge.id());
        }

        Some(result)
    }

    fn compute_roots<T,U>(graph : &Graph<T, U>, root_idx : NodeIndex) -> HashSet<NodeIndex> {
	let mut roots = HashSet::new();

	let mut gen : usize = 0;
        let mut seen = HashMap::<NodeIndex, usize>::new();
	for node in graph.node_indices() {
            if node == root_idx {
                continue;
            }
            gen += 1;
	    let mut work = vec![node];
	    while !work.is_empty() {
                let id = work.pop().unwrap();
                if let Some(when) = seen.get(&id) {
		    if *when == gen {
                        // Seen in the same generation -- we found a cycle.
                        // Randomly pick this node as the root.
			roots.insert(id);
                        // And stop the traversal, since otherwise we might
                        // pick multiple roots in this generation and they'd
                        // all be part of the same cycle.
                        break;
                    } else {
                        // We found a path to an older generation, so this node
                        // is reachable by that older generation's root.
                        continue;
                    }
                }

 		seen.insert(id, gen);
		let mut any_callers = false;
		for caller in graph.neighbors(id) {
		    any_callers = true;
		    work.push(caller);
                }
		if !any_callers {
		    roots.insert(id);
                }
            }
        }
	roots
    }

    pub fn roots(&mut self) -> Vec<NodeIndex> {
        if let Some(roots) = &self.roots {
	    return roots.iter().map(|&x| x).collect();
        }

        self.root = self.add_function("<root>");

        let roots = Callgraph::compute_roots(&self.caller_graph, self.root);
        let result = roots.iter().map(|&x| x).collect();
        self.roots = Some(roots);

        for root in self.roots() {
            self.add_edge(self.root, root, PropertySet { all: 0, any: 0 });
        }

        result
    }

    pub fn sinks(&mut self) -> Vec<NodeIndex> {
        if let Some(sinks) = &self.sinks {
	    return sinks.iter().map(|&x| x).collect();
        }

        self.sink = self.add_function("<sink>");

        let sinks = Callgraph::compute_roots(&self.graph, self.sink);
        let result = sinks.iter().map(|&x| x).collect();
        self.sinks = Some(sinks);

        for sink in self.sinks() {
            self.add_edge(sink, self.sink, PropertySet { all: 0, any: 0 });
        }

        result
    }
}
