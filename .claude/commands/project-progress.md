---
description: 基于git history以及相关文档列出当前项目进度
allowed-tools: Bash, Read, Grep, Write
---

# 项目进度报告：$ARGUMENTS

## 最近Git提交历史
!`git log --oneline -20 --graph --decorate`

## 当前分支和状态
!`git branch --show-current`
!`git status --short`

## 项目计划和目标
@context/plan.md

## 已实现的功能
通过分析代码结构和Git历史，列出已经完成的功能：

!`git log -n 25 --name-only --pretty="" | sort -u`

## 待实现的功能
根据plan.md和代码现状，分析待实现的功能。

## 项目依赖和构建状态
@Cargo.toml

!`cargo check 2>&1 | head -10`

使用Write工具生成详细的项目进度报告markdown文件，文件名为`progress-report-$(date +%Y%m%d-%H%M%S).md`，包含：
1. 项目概览
2. 已完成功能（基于Git历史和代码分析）
3. 待实现功能（基于计划文档）
4. 最近的提交和活动
5. 下一步建议
