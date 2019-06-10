mod hazard;
use hazard::load_graph;

mod callgraph;
use callgraph::Callgraph;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate cpython;

use cpython::{PyResult, Python, PyObject, PythonObject};
use std::cell;

py_class!(class HazGraph |py| {
    data callgraph: cell::RefCell<Callgraph>;

    def __new__(_cls, filename: &str) -> PyResult<HazGraph> {
        let callgraph = load_graph(filename, 0);
        let callgraph = callgraph.unwrap();
        HazGraph::create_instance(py, cell::RefCell::new(callgraph))
    }

    def resolve(&self, query: &str) -> PyResult<Vec<usize>> {
        let cg = self.callgraph(py).borrow();
        match cg.resolve(query) {
            None => Ok(vec![]),
            Some(matches) => Ok(matches.iter().map(|&x| x.index()).collect())
        }
    }
});

py_module_initializer!(hazgraph, inithazgraph, PyInit_hazgraph, |py, m| {
    m.add(py, "__doc__", "Python wrapper for Callgraph.")?;
    m.add_class::<HazGraph>(py)?;
    Ok(())
});
