// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Recording Storage
///
/// Handles persistence of recorded traffic to disk
use super::*;
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

pub struct RecordingStorage {
    base_dir: PathBuf,
}

impl RecordingStorage {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Save recording and flows to disk
    pub fn save(&self, recording: &Recording, flows: &[RecordedFlow]) -> Result<()> {
        // Ensure directory exists
        std::fs::create_dir_all(&self.base_dir)?;

        let file_path = &recording.file_path;

        // Write flows
        if recording.compressed {
            self.save_compressed(file_path, recording, flows)?;
        } else {
            self.save_uncompressed(file_path, recording, flows)?;
        }

        println!("💾 Saved recording to: {:?}", file_path);

        Ok(())
    }

    /// Save uncompressed
    fn save_uncompressed(
        &self,
        file_path: &Path,
        recording: &Recording,
        flows: &[RecordedFlow],
    ) -> Result<()> {
        let file = File::create(file_path)?;
        let writer = BufWriter::new(file);

        let data = RecordingData {
            metadata: recording.clone(),
            flows: flows.to_vec(),
        };

        serde_json::to_writer(writer, &data)?;

        Ok(())
    }

    /// Save compressed (gzip)
    fn save_compressed(
        &self,
        file_path: &Path,
        recording: &Recording,
        flows: &[RecordedFlow],
    ) -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let file_path_gz = file_path.with_extension("json.gz");
        let file = File::create(&file_path_gz)?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut writer = BufWriter::new(encoder);

        let data = RecordingData {
            metadata: recording.clone(),
            flows: flows.to_vec(),
        };

        serde_json::to_writer(&mut writer, &data)?;
        let encoder = writer
            .into_inner()
            .map_err(|e| anyhow::anyhow!("Failed to flush buffer: {}", e))?;
        encoder.finish()?;

        Ok(())
    }

    /// Load recording from disk
    pub fn load(&self, recording: &Recording) -> Result<Vec<RecordedFlow>> {
        let file_path = &recording.file_path;

        let flows = if recording.compressed {
            self.load_compressed(file_path)?
        } else {
            self.load_uncompressed(file_path)?
        };

        Ok(flows)
    }

    /// Maximum file size to load (256 MB)
    const MAX_LOAD_SIZE: u64 = 256 * 1024 * 1024;

    /// Load uncompressed
    fn load_uncompressed(&self, file_path: &Path) -> Result<Vec<RecordedFlow>> {
        let metadata = std::fs::metadata(file_path)?;
        if metadata.len() > Self::MAX_LOAD_SIZE {
            anyhow::bail!(
                "Recording file too large ({} bytes, max {})",
                metadata.len(),
                Self::MAX_LOAD_SIZE
            );
        }

        let file = File::open(file_path)?;
        let reader = BufReader::new(file);

        let data: RecordingData = serde_json::from_reader(reader)?;

        Ok(data.flows)
    }

    /// Load compressed
    fn load_compressed(&self, file_path: &Path) -> Result<Vec<RecordedFlow>> {
        use flate2::read::GzDecoder;

        let file_path_gz = file_path.with_extension("json.gz");
        let metadata = std::fs::metadata(&file_path_gz)?;
        if metadata.len() > Self::MAX_LOAD_SIZE {
            anyhow::bail!(
                "Compressed recording file too large ({} bytes, max {})",
                metadata.len(),
                Self::MAX_LOAD_SIZE
            );
        }

        let file = File::open(&file_path_gz)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::new(decoder);

        let data: RecordingData = serde_json::from_reader(reader)?;

        Ok(data.flows)
    }

    /// Load only metadata
    pub fn load_metadata(&self, recording_id: &str) -> Result<Recording> {
        // Reject path traversal attempts
        if recording_id.contains('/') || recording_id.contains('\\') || recording_id.contains("..")
        {
            anyhow::bail!("Invalid recording_id: must not contain path separators or '..'");
        }

        // Try both compressed and uncompressed
        let file_path = self.base_dir.join(format!("{}.json", recording_id));
        let file_path_gz = self.base_dir.join(format!("{}.json.gz", recording_id));

        if file_path_gz.exists() {
            self.load_metadata_from_file(&file_path_gz, true)
        } else if file_path.exists() {
            self.load_metadata_from_file(&file_path, false)
        } else {
            anyhow::bail!("Recording not found: {}", recording_id)
        }
    }

    fn load_metadata_from_file(&self, file_path: &Path, compressed: bool) -> Result<Recording> {
        if compressed {
            use flate2::read::GzDecoder;

            let file = File::open(file_path)?;
            let decoder = GzDecoder::new(file);
            let reader = BufReader::new(decoder);

            let data: MetadataOnly = serde_json::from_reader(reader)?;
            Ok(data.metadata)
        } else {
            let file = File::open(file_path)?;
            let reader = BufReader::new(file);

            let data: MetadataOnly = serde_json::from_reader(reader)?;
            Ok(data.metadata)
        }
    }

    /// List all recordings
    pub fn list(&self) -> Result<Vec<Recording>> {
        let mut recordings = Vec::new();

        if !self.base_dir.exists() {
            return Ok(recordings);
        }

        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let extension = path.extension().and_then(|e| e.to_str());

                // Check for .json or .json.gz files
                let is_recording = extension == Some("json")
                    || (extension == Some("gz") && path.to_string_lossy().ends_with(".json.gz"));

                if is_recording {
                    if let Ok(metadata) = self.load_metadata_from_path(&path) {
                        recordings.push(metadata);
                    }
                }
            }
        }

        // Sort by start time (newest first)
        recordings.sort_by_key(|a| std::cmp::Reverse(a.start_time));

        Ok(recordings)
    }

    fn load_metadata_from_path(&self, path: &Path) -> Result<Recording> {
        let compressed = path.to_string_lossy().ends_with(".gz");
        self.load_metadata_from_file(path, compressed)
    }

    /// Delete a recording
    pub fn delete(&self, recording_id: &str) -> Result<()> {
        // Reject path traversal attempts
        if recording_id.contains('/') || recording_id.contains('\\') || recording_id.contains("..")
        {
            anyhow::bail!("Invalid recording_id: must not contain path separators or '..'");
        }

        let file_path = self.base_dir.join(format!("{}.json", recording_id));
        let file_path_gz = self.base_dir.join(format!("{}.json.gz", recording_id));

        if file_path_gz.exists() {
            std::fs::remove_file(file_path_gz)?;
        } else if file_path.exists() {
            std::fs::remove_file(file_path)?;
        } else {
            anyhow::bail!("Recording not found: {}", recording_id);
        }

        println!("🗑️  Deleted recording: {}", recording_id);

        Ok(())
    }

    /// Get total storage size
    pub fn total_size(&self) -> Result<u64> {
        let mut total = 0u64;

        if !self.base_dir.exists() {
            return Ok(0);
        }

        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }

        Ok(total)
    }

    /// Clean old recordings
    pub fn cleanup_old(&self, max_age_secs: u64) -> Result<usize> {
        let recordings = self.list()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let mut deleted = 0;

        for recording in recordings {
            let age = now.saturating_sub(recording.end_time);

            if age > max_age_secs {
                self.delete(&recording.id)?;
                deleted += 1;
            }
        }

        if deleted > 0 {
            println!("🧹 Cleaned up {} old recordings", deleted);
        }

        Ok(deleted)
    }
}

/// Combined data structure for serialization
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RecordingData {
    metadata: Recording,
    flows: Vec<RecordedFlow>,
}

/// Lightweight struct that only deserializes metadata, skipping flows entirely.
#[derive(serde::Deserialize)]
struct MetadataOnly {
    metadata: Recording,
    #[serde(skip)]
    #[allow(dead_code)]
    flows: (),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use tempfile::TempDir;

    #[test]
    fn test_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RecordingStorage::new(temp_dir.path().to_path_buf());
        assert!(storage.base_dir.exists());
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RecordingStorage::new(temp_dir.path().to_path_buf());

        let recording = Recording {
            id: "test-1".to_string(),
            name: "Test Recording".to_string(),
            source_cluster: "test".to_string(),
            start_time: 1000,
            end_time: 2000,
            flow_count: 1,
            total_bytes: 1024,
            namespaces: vec!["default".to_string()],
            services: vec![],
            file_path: temp_dir.path().join("test-1.json"),
            compressed: false,
        };

        let flows = vec![RecordedFlow {
            timestamp: 1500,
            offset_ms: 500,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            src_port: 12345,
            dst_port: 80,
            protocol: 6,
            src_identity: 100,
            dst_identity: 200,
            src_namespace: "default".to_string(),
            dst_namespace: "default".to_string(),
            src_labels: HashMap::new(),
            dst_labels: HashMap::new(),
            verdict: PolicyVerdict::Allow,
            bytes: 1024,
            packets: 10,
            http_method: None,
            http_path: None,
            http_status: None,
        }];

        // Save
        storage.save(&recording, &flows).unwrap();

        // Load
        let loaded_flows = storage.load(&recording).unwrap();
        assert_eq!(loaded_flows.len(), 1);
        assert_eq!(loaded_flows[0].src_port, 12345);
    }

    #[test]
    fn test_list_recordings() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RecordingStorage::new(temp_dir.path().to_path_buf());

        // Create a test recording
        let recording = Recording {
            id: "test-2".to_string(),
            name: "Test".to_string(),
            source_cluster: "test".to_string(),
            start_time: 1000,
            end_time: 2000,
            flow_count: 0,
            total_bytes: 0,
            namespaces: vec![],
            services: vec![],
            file_path: temp_dir.path().join("test-2.json"),
            compressed: false,
        };

        storage.save(&recording, &[]).unwrap();

        let recordings = storage.list().unwrap();
        assert_eq!(recordings.len(), 1);
        assert_eq!(recordings[0].id, "test-2");
    }
}
