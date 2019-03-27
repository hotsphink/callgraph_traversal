extern crate petgraph;

use callgraph::Callgraph;
use petgraph::graph::NodeIndex;
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind};
use std::io::prelude::*;

pub fn load_graph(filename : &str, line_limit : u32) -> Result<Callgraph, Error> {
    let mut cg = Callgraph::new();

    let file = try!(File::open(filename));
    let mut reader = BufReader::new(file);

    fn error(message : &str) -> Result<Callgraph, Error> {
        return Err(Error::new(ErrorKind::Other, message));
    };

    let mut lineno = 1;
    let mut line = String::with_capacity(4000);
    loop {
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => (),
            Err(e) => {
                println!("Failed to read line {}: {}", lineno, e);
                return Err(e);
            }
        };

        match line.chars().nth(0) {
            Some('#') => {
                let space = match line.find(' ') {
                    Some(pos) => pos,
                    None => {
                        println!("Invalid format at {}: '{}'", lineno, line);
                        return error(&format!("Invalid format on line {}", lineno));
                    }
                };
                let function : &str = &line[1..space];
                match function.parse::<u32>() {
                    Ok(num) => {
                        let func = &line[(space+1)..line.len()-1];
                        let index = cg.add_function(func);
                        assert!(num as usize == index.index());
                    },
                    Err(e) => {
                        return error(&format!("Invalid number at {}: {}", lineno, e));
                    }
                }
            },
            Some('D')|Some('R') => {
                let mut iter = (&line[2..]).split_whitespace();
                let mut src = iter.next().expect("missing src function id");
                let mut dst = iter.next().expect("missing dst function id");
                let mut limit = 0;
                if &src[0..1] == "/" {
                    src = dst;
                    dst = iter.next().expect("missing dst function id");
                    limit = src[1..].parse().expect(format!("malformed limit {} on line {}", src, lineno).as_ref());
                };
                if src == "SUPPRESS_GC" {
                    src = dst;
                    dst = iter.next().expect("missing dst function id");
                    limit = 1;
                };

                let src : u32 = src.parse().expect(format!("malformed function id on line {}", lineno).as_ref());
                let dst : u32 = dst.parse().expect("malformed function id");
                let src = NodeIndex::new(src as usize);
                let dst = NodeIndex::new(dst as usize);
                cg.add_edge(src, dst, limit);
            },
            Some('=') => { // Unmangled name (one of them)
                let wtf = &line[2..];
                let space = match wtf.find(' ') {
                    Some(pos) => pos + 2,
                    None => {
                        println!("Invalid format at {}: '{}'", lineno, line);
                        return error("Invalid format");
                    }
                };
                let id : usize = line[2..space].parse().expect(format!("malformed function id on line {}", lineno).as_ref());
                cg.add_unmangled_name(id, &line[space+1..line.len()-1]);
            },
            Some('F') => {}, // Field call
            Some('I') => {}, // Indirect call
            Some('T') => {}, // Tag
            Some('V') => {}, // virtual method
            Some(_) => { panic!("Unhandled leading character at line {}", lineno) },
            None => {}
        }

        lineno += 1;
        if line_limit > 0 && lineno > line_limit { break; }
        line.clear();
    };

    println!("Final lineno = {}", lineno);

    return Ok(cg);
}
