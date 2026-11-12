#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Region {
    pub fn between(a: Point, b: Point, w: i32, h: i32) -> Self {
        let x = a.x.min(b.x).clamp(0, w);
        let y = a.y.min(b.y).clamp(0, h);
        Self {
            x,
            y,
            w: a.x.max(b.x).clamp(0, w) - x,
            h: a.y.max(b.y).clamp(0, h) - y,
        }
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }
    pub fn valid(self) -> bool {
        self.w > 1 && self.h > 1
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverse_and_clip() {
        assert_eq!(
            Region::between(Point { x: 120, y: 90 }, Point { x: -5, y: 10 }, 100, 80),
            Region {
                x: 0,
                y: 10,
                w: 100,
                h: 70
            }
        );
    }
    #[test]
    fn exclusive_edge() {
        let r = Region {
            x: 2,
            y: 2,
            w: 10,
            h: 10,
        };
        assert!(r.contains(Point { x: 2, y: 2 }));
        assert!(!r.contains(Point { x: 12, y: 5 }));
    }
    #[test]
    fn empty_selection() {
        assert!(!Region::between(Point { x: 1, y: 1 }, Point { x: 1, y: 9 }, 20, 20).valid());
    }
}
impl Region {
    pub fn hit(self, p: Point) -> u8 {
        if !self.valid()
            || p.x < self.x - 6
            || p.x > self.x + self.w + 6
            || p.y < self.y - 6
            || p.y > self.y + self.h + 6
        {
            return 0;
        }
        let mut edge = 0;
        if (p.x - self.x).abs() <= 6 {
            edge |= 1;
        } else if (p.x - self.x - self.w).abs() <= 6 {
            edge |= 2;
        }
        if (p.y - self.y).abs() <= 6 {
            edge |= 4;
        } else if (p.y - self.y - self.h).abs() <= 6 {
            edge |= 8;
        }
        if edge == 0 && self.contains(p) {
            16
        } else {
            edge
        }
    }
    pub fn adjusted(self, mode: u8, dx: i32, dy: i32, w: i32, h: i32) -> Self {
        if mode == 16 {
            return Self {
                x: (self.x + dx).clamp(0, (w - self.w).max(0)),
                y: (self.y + dy).clamp(0, (h - self.h).max(0)),
                ..self
            };
        }
        let mut l = self.x;
        let mut t = self.y;
        let mut r = l + self.w;
        let mut b = t + self.h;
        if mode & 1 != 0 {
            l = (l + dx).clamp(0, r - 2);
        }
        if mode & 2 != 0 {
            r = (r + dx).clamp(l + 2, w);
        }
        if mode & 4 != 0 {
            t = (t + dy).clamp(0, b - 2);
        }
        if mode & 8 != 0 {
            b = (b + dy).clamp(t + 2, h);
        }
        Self {
            x: l,
            y: t,
            w: r - l,
            h: b - t,
        }
    }
}
#[cfg(test)]
mod adjustment_tests {
    use super::*;
    #[test]
    fn move_resize_clamp() {
        let r = Region {
            x: 10,
            y: 10,
            w: 50,
            h: 50,
        };
        assert_eq!(
            r.adjusted(16, 100, -30, 100, 100),
            Region { x: 50, y: 0, ..r }
        );
        assert_eq!(r.adjusted(1, 99, 0, 100, 100).w, 2);
        assert_eq!(r.hit(Point { x: 10, y: 10 }), 5);
        assert_eq!(r.hit(Point { x: 30, y: 30 }), 16);
    }
}
