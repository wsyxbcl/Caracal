//! Morris reviews ONE exemplar per repeated detection, not every crop. Under
//! that workflow the cost of a parameter is the number of GROUPS, not the
//! number of candidates — so this prints both.
use rde_core::{find_suspicious, MdDocument, RdeOptions};

fn main() {
    let path = std::env::args().nth(1).expect("usage: exemplars <md.json>");
    let doc = MdDocument::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
    println!("{:>12} {:>8} {:>12} {:>14}", "setting", "groups", "candidates", "candidates/group");
    let show = |name: String, opts: RdeOptions| {
        let g = find_suspicious(&doc, &opts);
        let c: usize = g.iter().map(|x| x.instances.len()).sum();
        println!("{:>12} {:>8} {:>12} {:>14.1}", name, g.len(), c,
                 if g.is_empty() { 0.0 } else { c as f64 / g.len() as f64 });
    };
    for occ in [5usize, 10, 20, 50] {
        show(format!("occurrence {occ}"), RdeOptions { occurrence_threshold: occ, ..Default::default() });
    }
    for iou in [0.95f32, 0.9, 0.85, 0.8, 0.7] {
        show(format!("iou {iou:.2}"), RdeOptions { iou_threshold: iou, ..Default::default() });
    }
}
