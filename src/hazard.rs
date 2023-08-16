use crate::callgraph::{Callgraph, PropertySet};
use json;
use petgraph::graph::NodeIndex;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind};
use std::io::prelude::*;
use std::fmt;

#[derive(Debug)]
pub enum LoadError {
    IOError(Error),
    FormatError(u32, String)
}

impl std::error::Error for LoadError {}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LoadError::IOError(e) => write!(f, "I/O error: {}", e),
            LoadError::FormatError(lineno, s) => write!(f, "Format error on line {}: {}", lineno, s)
        }
    }
}

impl From<Error> for LoadError {
    fn from(e: Error) -> Self {
        LoadError::IOError(e)
    }
}

fn parse_proptable(text : &str, lineno : u32, cg : &mut Callgraph) -> Result<(), LoadError> {
    // There must be a more idiomatic way of doing this.
    let json = match json::parse(text) {
        Err(_) => return Err(LoadError::FormatError(lineno, "Bad JSON info".to_string())),
        Ok(x) => x,
    };
    let proptable = match &json["Properties"] {
        json::JsonValue::Object(x) => x,
        _ => return Err(LoadError::FormatError(lineno, "'Properties' key required in JSON info".to_string())),
    };
    for (k, name) in proptable.iter() {
        match k.parse::<u32>() {
            Err(_) => return Err(LoadError::FormatError(lineno, "Bad property bit number".to_string())),
            Ok(num) => {
                cg.property_names.insert(num, name.to_string());
            }
        }
    };
    Ok(())
}

pub fn load_graph(filename : &str, line_limit : u32) -> Result<Callgraph, LoadError> {
    let mut cg = Callgraph::new();
    let file = File::open(filename)?;
    let mut reader = BufReader::new(file);

    fn error(message : &str) -> Result<Callgraph, Error> {
        Err(Error::new(ErrorKind::Other, message))
    }

    let mut indirects = Vec::<(u32, String, PropertySet)>::new();

    let mut lineno = 0;
    let mut line = String::with_capacity(4000);
    loop {
        line.clear();
        match reader.read_line(&mut line)? {
            0 => break,
            _ => (),
        };
        lineno += 1;

        match line.chars().next() {
            Some('#') => {
                let space = match line.find(' ') {
                    Some(pos) => pos,
                    None => return Err(LoadError::FormatError(lineno, line)),
                };
                let function : &str = &line[1..space];
                match function.parse::<u32>() {
                    Ok(num) => {
                        let func = &line[(space+1)..line.len()-1];
                        let index = cg.add_function(func);
                        assert!(num as usize == index.index());
                    },
                    Err(_) => return Err(LoadError::FormatError(lineno, function.to_owned())),
                }
            },
            Some('D')|Some('R') => {
                let mut iter = (&line[2..]).split_whitespace();
                let mut src = iter.next().expect("missing src function id");
                let mut dst = iter.next().expect("missing dst function id");
                let mut limit = PropertySet { all: 0, any: 0 };
                if let Some(colon) = src.find(':') {
                    let all : u32 = src[0..colon].parse().unwrap_or_else(|_| panic!("malformed 'all:any' {} on line {}", src, lineno));
                    let any : u32 = src[colon+1..].parse().unwrap_or_else(|_| panic!("malformed 'all:any' {} on line {}", src, lineno));
                    limit = PropertySet { all, any };
                    src = dst;
                    dst = iter.next().expect("missing dst function id");
                } else if &src[0..1] == "/" {
                    let bits : u32 = src[1..].parse().unwrap_or_else(|_| panic!("malformed limit {} on line {}", src, lineno));
                    limit = PropertySet { all: bits, any: bits };
                    src = dst;
                    dst = iter.next().expect("missing dst function id");
                };
                if src == "SUPPRESS_GC" {
                    src = dst;
                    dst = iter.next().expect("missing dst function id");
                    limit = PropertySet { all: 1, any: 1 };
                };

                let src : u32 = src.parse().unwrap_or_else(|_| panic!("malformed function id on line {}", lineno));
                let dst : u32 = dst.parse().expect("malformed function id");
                let src = NodeIndex::new(src as usize);
                let dst = NodeIndex::new(dst as usize);
                cg.add_edge(src, dst, limit);
            },
            Some('=') => { // Unmangled name (one of them)
                let wtf = &line[2..];
                let space = match wtf.find(' ') {
                    Some(pos) => pos + 2,
                    None => return Err(LoadError::FormatError(lineno, line)),
                };
                let id : usize = line[2..space].parse().unwrap_or_else(|_| panic!("malformed function id on line {}", lineno));
                cg.add_unmangled_name(id, &line[space+1..line.len()-1]);
            },
            Some('F') => {}, // Field call
            Some('I') => { // Indirect call
                let mut len = 2;
                let mut iter = (&line[2..]).split_whitespace();
                let mut src = iter.next().expect("missing src function id");
                len += src.len();
                let mut limit = 0;
                if &src[0..1] == "/" {
                    limit = src[1..].parse().unwrap_or_else(|_| panic!("malformed limit {} on line {}", src, lineno));
                    src = iter.next().expect("missing src function id");
                    len += 1 + src.len();
                }
                let dst = &line[1+len..line.len()-1];
                let src : u32 = src.parse().unwrap_or_else(|_| panic!("malformed function id on line {}", lineno));
                // Have to defer generating a node for the indirect function
                // pointer, because otherwise it would change the numbering.
                indirects.push((src, dst.to_string(), PropertySet { all: limit, any: limit }));
            },
            Some('T') => {}, // Tag
            Some('V') => {}, // virtual method
            Some('!') => parse_proptable(&line[2..], lineno, &mut cg)?,
            Some(_) => return Err(LoadError::FormatError(lineno, "Unhandled leading character".to_string())),
            None => {}
        }

        if line_limit > 0 && lineno > line_limit { break; }
    };

    let mut seen = HashMap::<(&str,PropertySet),NodeIndex>::new();
    for (src, dst_name, limit) in &indirects {
        // For now, just leave the "VARIABLE " in the beginning.
        let key = (dst_name.as_ref(), *limit);
        let dst = match seen.entry(key) {
            Entry::Occupied(ent) => {
                *ent.get()
            },
            Entry::Vacant(ent) => {
                let dst = cg.add_function(dst_name);
                ent.insert(dst);
                dst
            }
        };
        cg.add_edge(NodeIndex::new(*src as usize), dst, *limit);
    }
    println!("{} indirects, {} distinct", indirects.len(), seen.len());

    let roots = &cg.roots();
    println!("found {} roots", roots.len());

    let sinks = &cg.sinks();
    println!("found {} sinks", sinks.len());

    println!("Final lineno = {}", lineno);

    Ok(cg)
}
