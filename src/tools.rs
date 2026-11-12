//! Stable command IDs bridge native controls; editor state uses typed tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum Tool {
    #[default]
    Select = 0,
    Pen = 1,
    Rectangle = 2,
    Ellipse = 3,
    Arrow = 4,
    Redact = 5,
    Text = 6,
    Mosaic = 13,
}
impl Tool {
    const KEYS: [(Self, u16); 8] = [
        (Self::Select, 0x56),
        (Self::Pen, 0x50),
        (Self::Rectangle, 0x52),
        (Self::Ellipse, 0x4f),
        (Self::Arrow, 0x41),
        (Self::Redact, 0x42),
        (Self::Text, 0x54),
        (Self::Mosaic, 0x4d),
    ];
    pub fn from_id(id: usize) -> Option<Self> {
        Self::KEYS
            .iter()
            .find(|(tool, _)| *tool as usize == id)
            .map(|(tool, _)| *tool)
    }
    pub fn from_key(key: u16) -> Option<Self> {
        Self::KEYS
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(tool, _)| *tool)
    }
}
pub const ORDER: [usize; 14] = [0, 1, 2, 3, 4, 6, 13, 5, 8, 9, 11, 10, 12, 14];
pub const TIPS: [&str; 15] = [
    "选区 V · 拖动调整",
    "画笔 P",
    "矩形 R",
    "椭圆 O",
    "箭头 A",
    "实色遮挡 B",
    "文字 T",
    "自定义颜色 C",
    "撤销  Ctrl+Z",
    "重做  Ctrl+Y",
    "保存  Ctrl+S",
    "贴图  Ctrl+T",
    "复制  Enter",
    "马赛克 M",
    "取消  Esc",
];
