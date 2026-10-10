// SPDX-License-Identifier: AGPL-3.0-or-later
//! Independent Bernstein sampling of the public geometry API.
#![forbid(unsafe_code)]
use atelier_geom::{AffineTransform, Path, Point};

fn point(x: f64, y: f64) -> Point { Point::try_new(x, y).unwrap() }
fn distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x()-a.x(), b.y()-a.y());
    let norm = dx*dx+dy*dy;
    let t = if norm == 0.0 { 0.0 } else {
        (((p.x()-a.x())*dx+(p.y()-a.y())*dy)/norm).clamp(0.0,1.0)
    };
    (p.x()-a.x()-t*dx).hypot(p.y()-a.y()-t*dy)
}
fn main() {
    let mut state = 14060070_u64;
    let mut random = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 32) as i32 as f64) / (i32::MAX as f64) * 1000.0
    };
    for index in 0..200 {
        let a = point(random(),random());
        let b = point(random(),random());
        let c = point(random(),random());
        let d = if index % 7 == 0 { a } else { point(random(),random()) };
        let mut path = Path::new();
        path.move_to(a);
        let cubic = index % 2 == 0;
        if cubic { path.cubic_to(b,c,d).unwrap(); } else { path.quadratic_to(b,d).unwrap(); }
        let bounds = path.bounds().unwrap().unwrap();
        let flattened = path.flatten(0.125).unwrap();
        let points = &flattened[0].points;
        assert_eq!(points.first(),Some(&a));
        assert_eq!(points.last(),Some(&d));
        for step in 0..=1000 {
            let t = step as f64 / 1000.0;
            let u = 1.0-t;
            let sample = if cubic {
                point(u*u*u*a.x()+3.0*u*u*t*b.x()+3.0*u*t*t*c.x()+t*t*t*d.x(),
                      u*u*u*a.y()+3.0*u*u*t*b.y()+3.0*u*t*t*c.y()+t*t*t*d.y())
            } else {
                point(u*u*a.x()+2.0*u*t*b.x()+t*t*d.x(),u*u*a.y()+2.0*u*t*b.y()+t*t*d.y())
            };
            assert!(sample.x() >= bounds.left()-1e-8 && sample.x() <= bounds.right()+1e-8);
            assert!(sample.y() >= bounds.top()-1e-8 && sample.y() <= bounds.bottom()+1e-8);
            let nearest = points.windows(2).map(|s|distance(sample,s[0],s[1])).fold(f64::INFINITY,f64::min);
            assert!(nearest <= 0.125+1e-8,"curve {index}, t={t}, distance={nearest}");
        }
        let transform = AffineTransform::rotation(random()).unwrap()
            .then(AffineTransform::scale(0.75,1.25).unwrap()).unwrap()
            .then(AffineTransform::translation(random(),random()).unwrap()).unwrap();
        let recovered = transform.inverse().unwrap().apply(transform.apply(a).unwrap()).unwrap();
        assert!((recovered.x()-a.x()).abs() < 1e-8 && (recovered.y()-a.y()).abs() < 1e-8);
    }
    println!("independent Bernstein oracle: 200 curves, 200200 samples and affine inverses passed");
}
