#![feature(fnbox)]
/*!
 * This is some kind of a library for dataflow computation. It's still very experimental and may
 * become something completely different in the end.
 *
 * The end goal is to use it for procedural and generative art. It's inspired by Pure Data and
 * Max/MSP, but will probably have less focus on graphical programming. Modular live coding,
 * perhaps?
 */

extern crate num_complex;
#[macro_use]
extern crate serde_derive;
extern crate serde;

/// Describes the data structure of the computation graph.
pub mod graph;

/// Manages the data flow within the computation graph.
pub mod context;
