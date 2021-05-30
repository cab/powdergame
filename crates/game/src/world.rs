struct World {}

#[derive(Debug)]
struct Cells {
    // double buffering
    cells_a: CellsInner,
    cells_b: CellsInner,
    active: Active,
}

// which buffer is active
#[derive(Debug, Copy, Clone)]
enum Active {
    A,
    B,
}

impl Active {
    fn swap(&self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

impl Cells {
    fn inner_active(&self) -> &CellsInner {
        match self.active {
            Active::A => &self.cells_a,
            Active::B => &self.cells_b,
        }
    }

    fn inner_back(&self) -> &CellsInner {
        match self.active {
            Active::A => &self.cells_b,
            Active::B => &self.cells_a,
        }
    }

    pub fn update(&mut self) {
        self.active = self.active.swap();
    }
}

#[derive(Debug)]
struct CellsInner {
    width: u32,
    height: u32,
    cells: Vec<Cell>,
}

impl CellsInner {
    fn cell_at(&self, x: u32, y: u32) -> Option<&Cell> {
        let x_index = (self.height) * x;
        let y_index = y;
        let index = x_index + y_index;
        self.cells.get(index as usize)
    }
}

#[derive(Debug, Copy, Clone)]
enum Cell {}

// [nw, n, ne, w, c, e, sw, s, se]
type Neighborhood = [Cell; 9];

const NEIGHBORHOOD: [(i64, i64); 9] = [
    (-1, 1),
    (0, 1),
    (1, 1),
    (-1, 0),
    (0, 0),
    (1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
