//! 注册和 HTTP／性能对照共用的确定性负载；不包含原型专用入口。
pub fn path(shape: &str, index: usize) -> String {
    match shape {
        "static" => format!("/bench/item{index:05}"),
        "dynamic" => format!("/bench/item{index:05}/{{id}}"),
        "overlap" => {
            let pair = index / 2;
            if pair.is_multiple_of(2) == index.is_multiple_of(2) {
                format!("/bench/item{pair:05}/{{id}}")
            } else {
                format!("/bench/item{pair:05}/fixed")
            }
        }
        _ => panic!("未知路由负载"),
    }
}
pub fn queries(shape: &str, count: usize) -> Vec<(String, Option<usize>)> {
    let mut queries: Vec<_> = [0, count / 2, count - 1, count - 4]
        .into_iter()
        .map(|index| {
            let path = match shape {
                "static" => format!("/bench/item{index:05}"),
                "dynamic" => format!("/bench/item{index:05}/42"),
                _ => format!("/bench/item{:05}/fixed", index / 2),
            };
            (
                path,
                Some(if shape == "overlap" {
                    index / 2 * 2
                } else {
                    index
                }),
            )
        })
        .collect();
    queries.push(("/bench/missing".into(), None));
    queries.push((
        match shape {
            "static" => format!("/bench/item{:05}/extra", count - 1),
            "dynamic" => format!("/bench/item{:05}/", count - 1),
            _ => format!("/bench/item{:05}/", count / 2 - 1),
        },
        None,
    ));
    queries
}
