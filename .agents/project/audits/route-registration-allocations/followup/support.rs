use std::{hint::black_box, collections::HashSet, sync::Arc};
use openrust::{routing::Router,http::{Method,Request,HttpFuture,Response,HttpError},contracts::routing::Handler};
struct ProbeHandler;
impl Handler for ProbeHandler { fn handle<'a>(&'a self,_:&'a mut Request)->HttpFuture<'a,Result<Response,HttpError>> {Box::pin(async {Ok(Response::from("ok"))})} }
pub struct Work { case: String, paths: Vec<String>, router: Router, methods: Vec<Method>, handler:Arc<dyn Handler> }
impl Work {
 pub fn new(case: String) -> Self {
  let paths:Vec<_>=(0..128).map(|i| if case=="routes_dynamic" {format!("/perf/route/{i:04}/{{id}}") } else {format!("/perf/route/{i:04}")}).collect();
  let mut router=Router::new();
  if case.starts_with("match") { for p in &paths {router.get(p,||"ok").unwrap();} }
  let w=Self{case,paths,router,methods:vec![Method::GET,Method::HEAD],handler:Arc::new(ProbeHandler)};
  if !w.case.ends_with("cold") {w.run(1);} w
 }
 pub fn run(&self,n:u64) {
  for _ in 0..n {match self.case.as_str() {
   "routes_get"|"routes_get_cold"|"routes_any"|"routes_dynamic"|"routes_duplicate"=>{
    let mut r=Router::new();
    for p in &self.paths {
     match self.case.as_str() {
      "routes_any"=>{r.any(black_box(p),||"ok").unwrap();},
      "routes_dynamic"=>{r.get(black_box(p),||"ok").unwrap();},
      "routes_duplicate"=>{r.get("/same",||"ok").unwrap();},
      _=>{r.get(black_box(p),||"ok").unwrap();}
     }
    } black_box(r);
   },
   "prepare"=>for p in &self.paths {self.router.__audit_prepare(black_box(p),self.handler.clone());},
   "pattern"=>for p in &self.paths { Router::__audit_pattern(black_box(p)); },
   "regex"=>for p in &self.paths {black_box(regex::RegexBuilder::new(&format!(r"\A{}\z",regex::escape(black_box(p)))).dot_matches_new_line(true).build().unwrap());},
   "uri"=>for p in &self.paths {black_box([p.as_str()].into_iter().map(|s|s.trim_matches('/')).fold(String::new(),|mut s,p|{s.push('/');s.push_str(p);s}));},
   "method_vec"=>for _ in &self.paths {let mut m=black_box(&self.methods[..1]).to_vec();if m.contains(&Method::GET)&&!m.contains(&Method::HEAD){m.push(Method::HEAD);}black_box(m);},
   "method_metadata"=>for _ in &self.paths {black_box(black_box(&self.methods).iter().map(|m|m.as_str().to_owned()).collect::<Vec<_>>());},
   "method_set"=>for _ in &self.paths {black_box(black_box(&self.methods).iter().collect::<HashSet<_>>());},
   "handler"=>for _ in &self.paths {black_box(Arc::new(||"ok"));},
   "match_first"=>{black_box(self.router.__audit_match(black_box(&self.paths[0])));},
   "match_last"=>{black_box(self.router.__audit_match(black_box(&self.paths[127])));},
   "match_miss"=>{black_box(self.router.__audit_match(black_box("/missing")));},
   _=>panic!("unknown case")
  }}
 }
}
pub fn verify() {let mut r=Router::new();r.get("/get",||"ok").unwrap();r.any("/any",||"ok").unwrap();assert_eq!(r.get_routes().len(),2);println!("{{\"verified\":true}}");}
