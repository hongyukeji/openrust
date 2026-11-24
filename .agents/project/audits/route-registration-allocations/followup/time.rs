#[path="support.rs"] mod probe_support;
fn main() {
 let mut args=std::env::args().skip(1);
 let case=args.next().unwrap();if case=="verify" {probe_support::verify();return}
 let n=args.next().unwrap().parse::<u64>().unwrap();let work=probe_support::Work::new(case.clone());
 if !case.ends_with("cold") {work.run((n/10).max(1));}
 let start=std::time::Instant::now();work.run(n);let ns=start.elapsed().as_nanos();
 println!("{{\"case\":\"{case}\",\"iterations\":{n},\"elapsed_ns\":{ns}}}");
}
