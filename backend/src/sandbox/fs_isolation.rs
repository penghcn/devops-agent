use std::fs;
use std::path::{Path, PathBuf};

use super::path_check::{PathValidation, PathValidator};

/// 文件系统隔离配置
pub struct FsIsolationConfig {
    pub workspace_root: PathBuf,
    pub tmp_dir: PathBuf,
    pub output_dir: PathBuf,
    pub read_only_mounts: Vec<PathBuf>,
}

/// 文件系统隔离器，限制文件操作在安全边界内
pub struct FileSystemIsolator {
    config: FsIsolationConfig,
    validator: PathValidator,
}

impl FileSystemIsolator {
    pub fn new(config: FsIsolationConfig) -> Self {
        Self {
            validator: PathValidator::new(
                config.workspace_root.to_str().unwrap_or("/workspace"),
            ),
            config,
        }
    }

    /// 检查是否允许读取：workspace 内或只读挂载目录
    pub fn can_read(&self, path: &str) -> bool {
        let abs = Path::new(path);

        // 绝对路径：直接检查是否在只读挂载目录中
        if abs.is_absolute() {
            if self.config.read_only_mounts.iter().any(|m| abs.starts_with(m)) {
                return true;
            }
            // 绝对路径不在只读挂载中，走正常校验
            return self.validator.validate(path) == PathValidation::Ok;
        }

        // 相对路径：校验后检查是否在 workspace 内
        self.validator.validate(path) == PathValidation::Ok
    }

    /// 检查是否允许写入：仅限 output 目录
    pub fn can_write(&self, path: &str) -> bool {
        let abs = Path::new(path);

        // 绝对路径：检查是否在 output 目录
        if abs.is_absolute() {
            return abs.starts_with(&self.config.output_dir);
        }

        // 相对路径：检查解析后是否在 output 目录
        let resolved = self.config.output_dir.join(path);
        resolved.starts_with(&self.config.output_dir)
    }

    /// 解析为安全绝对路径
    pub fn resolve_path(&self, path: &str) -> Option<PathBuf> {
        let validation = self.validator.validate(path);
        if validation != PathValidation::Ok {
            return None;
        }

        let p = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.config.workspace_root.join(path)
        };
        Some(p)
    }

    /// 确保所需目录存在
    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.config.workspace_root)?;
        fs::create_dir_all(&self.config.tmp_dir)?;
        fs::create_dir_all(&self.config.output_dir)?;
        Ok(())
    }
}
