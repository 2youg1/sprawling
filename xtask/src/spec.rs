// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! `cargo xtask spec <crate>`: create a `<crate>-SPEC.md` skeleton with the
//! the seventeen sections a SPEC is written in.
//! Creation only — an existing SPEC is never overwritten (the SPEC is the
//! construction authority; regenerating it would erase decisions).

use std::path::Path;

use crate::report::XtaskError;

const SECTIONS: [&str; 19] = [
    "## 1 需求拆解",
    "## 2 验收标准",
    "## 3 假设与歧义",
    "## 4 现状分析",
    "## 5 权威信源",
    "## 6 命名统一",
    "## 7 模块边界\n\n**三件邻居的活，及它们各自的主人**（写「X 归 Y」而非「不做 X」：前者告诉施工者去哪，后者只告诉他别去哪里）：",
    "## 8 接口先行",
    "## 8.5 两个设计\n\n（两个实质不同的接口方案，按杠杆率与缝的位置比较；落选方案就地留痕。）",
    "## 9 工作流程",
    "## 10 实现逻辑",
    "## 11 边界枚举",
    "## 12 错误处理\n\n（逐码回答「能否让它不可能发生」——设计规则十。）",
    "## 13 依赖选型",
    "## 14 硬编码声明",
    "## 15 影响面",
    "## 16 测试与约束",
    "## 17 模型体验\n\n（入窗什么｜token 代价｜对 prefix 缓存的影响；无贡献则写「零字节，因为……」。）",
    "## 18 文档同步",
];

pub(crate) fn run(root: &Path, crate_name: Option<&str>) -> Result<String, XtaskError> {
    let name = crate_name.ok_or_else(|| XtaskError::Doc {
        file: "spec".to_owned(),
        msg: "usage: cargo xtask spec <crate>".to_owned(),
    })?;
    let dir = if name == "xtask" || name == "citysim" {
        root.join(name)
    } else {
        root.join("crates").join(name)
    };
    if !dir.is_dir() {
        return Err(XtaskError::Doc {
            file: name.to_owned(),
            msg: format!("no such crate directory: {}", dir.to_string_lossy()),
        });
    }
    let path = dir.join(format!("{name}-SPEC.md"));
    if path.exists() {
        return Ok(format!(
            "already exists, left untouched: {}",
            path.to_string_lossy()
        ));
    }
    let mut body = format!(
        "# {name}-SPEC.md\n\n> crate：`{name}`。本 SPEC 先于代码存在；实现不多不少地遵守本文。\n\
         > 骨架：apostle-sdd 十七节；按模块分章、每章自足（ARCHITECTURE.md §5）。\n\
         > 动手前先读所用工具与依赖的**官方文档或官方 agent 指南**，再写本文的接口节。\n"
    );
    for section in SECTIONS {
        body.push('\n');
        body.push_str(section);
        body.push('\n');
    }
    std::fs::write(&path, body).map_err(|source| XtaskError::Io {
        path: path.to_string_lossy().into_owned(),
        source,
    })?;
    Ok(format!("created {}", path.to_string_lossy()))
}
