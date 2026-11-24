use std::hint::black_box;
use openrust::{routing::Router,database::eloquent::factories::Faker};

pub struct Work {
 pub case: String,
 paths: Vec<String>,
 faker: Option<Faker>,
}
impl Work {
 pub fn new(case: String) -> Self {
  let paths = if (case.starts_with("routes") || case=="pattern" || case=="regex") {(0..128).map(|i|format!("/perf/route/{i:04}")).collect()} else {Vec::new()};
  let faker = if case.contains("format") {let f=Faker::new("en-US").unwrap();f.seed(1_u64);Some(f)} else {None};
  let w=Self {case,paths,faker};
  if !w.case.ends_with("cold") {w.run(1);}
  w
 }
 pub fn run(&self,n:u64) {
  match self.case.as_str() {
   "methods"=>for _ in 0..n {black_box(Router::__array_perf_methods());},
   "pattern"=>for _ in 0..n {for path in black_box(&self.paths) {Router::__profile_pattern(black_box(path));}},
   "regex"=>for _ in 0..n {for path in black_box(&self.paths) {Router::__profile_regex(black_box(path));}},
   "application_create"=>for _ in 0..n {black_box(openrust::foundation::Application::configure(black_box(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))).create().unwrap());},
   "noop_cold"=>for _ in 0..n {black_box(());},
   "routes_get"|"routes_any"=>for _ in 0..n {
    let mut r=Router::new();
    for p in black_box(&self.paths) {
     if self.case=="routes_any" {r.any(black_box(p.as_str()),||"ok").unwrap();}
     else {r.get(black_box(p.as_str()),||"ok").unwrap();}
    }
    black_box(r);
   },
   "faker_new"|"faker_new_cold"=>for _ in 0..n {black_box(Faker::new(black_box("en-US")).unwrap());},
   "faker_format"|"faker_format_cold"=>for _ in 0..n {black_box(self.faker.as_ref().unwrap().format(black_box("randomElement"),black_box(&[])).unwrap());},
   "elements"|"elements_cold"=>for _ in 0..n {black_box(Faker::__array_perf_elements());},
   _=>panic!("unknown case"),
  }
 }
}
pub fn verify() {
 let methods=Router::__array_perf_methods();
 assert_eq!(methods.map(|v|v.to_string()),["GET","HEAD","POST","PUT","PATCH","DELETE","OPTIONS","QUERY"]);
 let elems=Faker::__array_perf_elements();assert_eq!(elems.len(),3);
 for (v,s) in elems.iter().zip(["a","b","c"]) {assert_eq!(v.as_str(),Some(s));}
 let mut r=Router::new();r.get("/get",||"ok").unwrap();r.any("/any",||"ok").unwrap();
 let info=r.get_routes();assert!(info.iter().any(|v|v.uri=="get"));assert!(info.iter().any(|v|v.uri=="any"));
 println!("{{\"verified\":true,\"routes\":{}}}",info.len());
}
