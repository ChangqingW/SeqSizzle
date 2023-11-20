use interval::interval_set::{IntervalSet, ToIntervalSet};
use interval::ops::Width;
use gcollections::ops::set::{Union, Intersection};
use gcollections::ops::{Difference, Empty};

pub fn find_intersections<Bound: Width + num_traits::Num>(sets: &[IntervalSet<Bound>]) -> IntervalSet<Bound> {
    sets.iter()
        .enumerate()
        .flat_map(|(i, &ref x)| sets.iter().skip(i + 1).map(move |y| (x.clone(), y.clone())))
        .fold(IntervalSet::empty(), |acc, (j, k)| acc.union(&j.intersection(&k)))
}

#[test]
fn test_find_intersections() {
    let a: IntervalSet<usize> = vec![(1, 2), (4, 7), (9, 10)].to_interval_set();
    let b: IntervalSet<usize> = vec![(4, 6), (100, 110)].to_interval_set();
    let c: IntervalSet<usize> = vec![(8, 12), (105, 110)].to_interval_set();
    let sets = vec![a, b, c];
    // 4, 6  9,10  105,110
    assert_eq!(find_intersections(&sets), vec![(4, 6), (9, 10), (105, 110)].to_interval_set());
}

#[test]
fn test_difference() {
    let a: IntervalSet<usize> = vec![(1, 2), (4, 7), (9, 10)].to_interval_set();
    let b: IntervalSet<usize> = vec![(4, 6), (100, 110)].to_interval_set();
    assert_eq!(a.difference(&b), vec![(1, 2), (7, 7), (9, 10)].to_interval_set());
}
