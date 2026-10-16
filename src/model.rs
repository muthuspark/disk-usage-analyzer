use std::sync::Arc;

#[derive(Clone)]
pub struct Node {
    pub name: String,
    pub bytes: u64,
    pub items: u64,
    pub modified: String,
    pub restricted: bool,
    pub children: Arc<Vec<Node>>,
}

impl Node {
    pub fn empty(name: String) -> Self {
        Self {
            name,
            bytes: 0,
            items: 0,
            modified: String::new(),
            restricted: false,
            children: Arc::new(vec![]),
        }
    }

    pub fn at(&self, path: &[usize]) -> &Self {
        path.iter().fold(self, |node, &i| &node.children[i])
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else if bytes >= 1_000 {
        format!("{:.1} kB", bytes as f64 / 1e3)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
fn node(name: &str, bytes: u64, items: u64, children: Vec<Node>) -> Node {
    Node {
        name: name.into(),
        bytes,
        items,
        children: children.into(),
        modified: "Today".into(),
        restricted: false,
    }
}

// Deterministic fixture for chart tests only.
#[cfg(test)]
pub fn demo() -> Node {
    let gb = 1_000_000_000;
    let var = node(
        "var",
        29_800_000_000,
        200188,
        vec![
            node(
                "lib",
                21_400_000_000,
                190830,
                vec![
                    node(
                        "containers",
                        21_200_000_000,
                        175821,
                        vec![
                            node(
                                "storage",
                                21_000_000_000,
                                173002,
                                vec![
                                    node("overlay", 20_900_000_000, 170402, vec![]),
                                    node("volumes", 100_000_000, 2600, vec![]),
                                ],
                            ),
                            node("cache", 200_000_000, 2819, vec![]),
                        ],
                    ),
                    node("flatpak", 200_000_000, 15009, vec![]),
                ],
            ),
            node(
                "cache",
                4_600_000_000,
                8158,
                branches("packages", 4_600_000_000, 2, 1),
            ),
            node(
                "log",
                3_800_000_000,
                1200,
                branches("journal", 3_800_000_000, 4, 2),
            ),
        ],
    );
    let usr = node(
        "usr",
        11_400_000_000,
        237385,
        vec![
            node(
                "lib64",
                4_900_000_000,
                78200,
                branches("libraries", 4_900_000_000, 8, 2),
            ),
            node(
                "share",
                4_200_000_000,
                132000,
                branches("resources", 4_200_000_000, 7, 3),
            ),
            node(
                "bin",
                1_500_000_000,
                20185,
                branches("tools", 1_500_000_000, 5, 1),
            ),
            node("lib", 800_000_000, 7000, vec![]),
        ],
    );
    let mut root = node(
        "leopard",
        42_334_000_012,
        439840,
        vec![
            var,
            usr,
            node("opt", gb, 141, branches("applications", gb, 4, 2)),
            node("etc", 34_000_000, 2119, vec![]),
            node("root", 0, 0, vec![]),
            node("mnt", 12, 1, vec![]),
            node("srv", 0, 1, vec![]),
            node("media", 0, 1, vec![]),
            node("lost+found", 0, 0, vec![]),
            node("afs", 0, 1, vec![]),
        ],
    );
    Arc::make_mut(&mut root.children)[2].modified = "1 month".into();
    Arc::make_mut(&mut root.children)[4].restricted = true;
    Arc::make_mut(&mut root.children)[4].modified = "5 months".into();
    Arc::make_mut(&mut root.children)[8].restricted = true;
    Arc::make_mut(&mut root.children)[8].modified = "8 months".into();
    root
}

#[cfg(test)]
fn branches(prefix: &str, bytes: u64, count: u64, depth: u64) -> Vec<Node> {
    let weights: Vec<u64> = (0..count).map(|i| count - i).collect();
    let sum: u64 = weights.iter().sum();
    weights
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let n = bytes * w / sum;
            // Some files terminate at each level, producing the reference's irregular silhouette.
            node(
                &format!("{prefix}-{}", i + 1),
                n,
                100 + i as u64 * 31,
                if depth > 0 && i % 3 != 2 {
                    branches("files", n * 9 / 10, 3, depth - 1)
                } else {
                    vec![]
                },
            )
        })
        .collect()
}
