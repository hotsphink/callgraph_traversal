mod hazard;
use hazard::load_graph;

mod callgraph;
use callgraph::Callgraph;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate cpython;

use cpython::{PyResult, PyErr};
use cpython::exc;
use petgraph::graph::NodeIndex;
use std::cell;
use std::collections::HashSet;
use std::iter::FromIterator;

// impl cpython::ToPyObject for NodeIndex {
//     type ObjectType = PyInt;
//     fn to_py_object(&self, py: Python) -> Self::ObjectType {
//         PyInt::new(py, self.index())
//     }
// }

py_class!(class HazGraph |py| {
    data callgraph: cell::RefCell<Callgraph>;

    def __new__(_cls, filename: &str) -> PyResult<HazGraph> {
        let callgraph = load_graph(filename, 0);
        let callgraph = callgraph.unwrap();
        HazGraph::create_instance(py, cell::RefCell::new(callgraph))
    }

    def resolve(&self, query: &str) -> PyResult<Vec<usize>> {
        let cg = self.callgraph(py).borrow();
        if &query[0..1] == "#" {
            match query[1..].parse::<usize>() {
                Ok(n) => {
                    Ok(vec![n])
                },
                Err(_) =>
                    Err(PyErr::new::<exc::ValueError, _>(py, "invalid node id"))
            }
        } else {
            match cg.resolve(query) {
                None => Ok(vec![]),
                Some(matches) => Ok(matches.iter().map(|&x| x.index()).collect())
            }
        }
    }

    def callees(&self, func: usize) -> PyResult<Vec<usize>> {
        let cg = self.callgraph(py).borrow();
        let callees = cg.callees(NodeIndex::new(func));
        Ok(callees.iter().map(|&x| x.index()).collect())
    }

    def callers(&self, func: usize) -> PyResult<Vec<usize>> {
        let cg = self.callgraph(py).borrow();
        let callers = cg.callers(NodeIndex::new(func));
        Ok(callers.iter().map(|&x| x.index()).collect())
    }

    def route(&self, src: usize, goal: Vec<usize>, avoid: Vec<usize>, avoid_props: u32) -> PyResult<Vec<usize>> {
        let cg = self.callgraph(py).borrow();
        let src = NodeIndex::new(src);
        let goal : Vec<NodeIndex> = goal.iter().map(|&x| NodeIndex::new(x)).collect();
        let goal = HashSet::from_iter(goal);
        let avoid : Vec<NodeIndex> = avoid.iter().map(|&x| NodeIndex::new(x)).collect();
        let avoid = HashSet::from_iter(avoid);

        match cg.any_route(src, &goal, &avoid, avoid_props) {
            None => Ok(vec![]),
            Some(route) => Ok(route.iter().map(|&x| x.index()).collect())
        }
    }

    def names(&self, func: usize) -> PyResult<Vec<String>> {
        let cg = self.callgraph(py).borrow();
        let names = cg.names(NodeIndex::new(func));
        Ok(names.iter().map(|&x| x.to_string()).collect())
    }

    // Err(PyErr::new::<exc::TypeError, _>(py, "unimplemented"))
});

py_module_initializer!(hazgraph, inithazgraph, PyInit_hazgraph, |py, m| {
    m.add(py, "__doc__", "Python wrapper for Callgraph.")?;
    m.add_class::<HazGraph>(py)?;
    Ok(())
});
