use std::env;
use std::path::Path;
use std::fs::File;
use std::io::Write;
use std::thread;

use reqwest;
use cloud_storage::Object;
use rusoto_core::Region;
use rusoto_core::credential::AwsCredentials;
use rusoto_s3::{S3, S3Client, ListObjectsRequest, GetObjectRequest};
use rusoto_s3::util::{PreSignedRequest, PreSignedRequestOption};
use tokio::runtime::Runtime;

pub trait RomManager {
    fn pull_roms(&mut self, emu_type: &str, system_name: &str) -> Result<(), String>;
}

pub struct AwsRomManager {
    base_roms_path: String,
    creds: AwsCredentials,
    region: Region,
    bucket: String,
    s3_client: S3Client,
    runtime: Runtime
}

impl AwsRomManager {
    pub fn create(roms_path: &str) -> impl RomManager {
        let region = Region::ApNortheast2;
        AwsRomManager {
            base_roms_path: String::from(roms_path),
            creds: AwsCredentials::new(
                env::var("AWS_ACCESS_KEY_ID").unwrap(),
                env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
                None,
                None
            ),
            region: region.to_owned(),
            bucket: "oraksil".to_owned(),
            s3_client: S3Client::new(region.to_owned()),
            runtime: Runtime::new().unwrap()
        }
    }

    fn generate_presigned_url(&self, obj_key: &str) -> String {
        let req = GetObjectRequest {
            bucket: self.bucket.to_owned(),
            key: obj_key.to_owned(),
            ..Default::default()
        };
        req.get_presigned_url(&self.region, &self.creds, &PreSignedRequestOption::default())
    }

    fn download_objects(&self, obj_keys: &Vec<String>) {
        let join_handles: Vec<std::thread::JoinHandle<_>> = obj_keys.into_iter()
            .map(|k| { 
                let filename = Path::new(&k).file_name().unwrap();
                println!("downloading... {:?}", filename);

                let url = self.generate_presigned_url(&k);
                let path = Path::new(&self.base_roms_path).join(filename);
                thread::spawn(move || gcp_download_url_to_path(&url, &path))
            })
            .collect();

        for h in join_handles {
            let _ = h.join();
        }
    }

    fn list_rom_objects(&mut self, emu_type: &str, system_name: &str) -> Vec<String> {
        let req = ListObjectsRequest {
            bucket: self.bucket.to_owned(),
            prefix: Some(format!("games/{}/{}", emu_type, system_name)),
            ..Default::default()
        };

        self.runtime.block_on(self.s3_client.list_objects(req)).unwrap()
            .contents.unwrap()
            .into_iter()
            .map(|o| o.key.unwrap())
            .collect()
    }
}

impl RomManager for AwsRomManager {
    fn pull_roms(&mut self, emu_type: &str, system_name: &str) -> Result<(), String> {
        let rom_objects = self.list_rom_objects(&emu_type, &system_name);
            
        self.download_objects(&rom_objects);

        println!("download done..");

        Ok(())
    }
}

pub struct GcpRomManager {
    base_roms_path: String
}

impl GcpRomManager {
    pub fn create(roms_path: &str) -> impl RomManager {
        GcpRomManager {
            base_roms_path: String::from(roms_path)
        }
    }

    fn download_objects(&self, objects: &Vec<Object>) {
        let join_handles: Vec<std::thread::JoinHandle<_>> = objects.into_iter()
            .map(|o| { 
                println!("downloading... {}", o.name);

                let url = o.download_url(120).unwrap();
                let filename = Path::new(&o.name).file_name().unwrap();
                let path = Path::new(&self.base_roms_path).join(filename);
                thread::spawn(move || gcp_download_url_to_path(&url, &path))
            })
            .collect();

        for h in join_handles {
            let _ = h.join();
        }
    }

    fn list_rom_objects(&self, emu_type: &str, system_name: &str) -> Vec<Object> {
        let prefix = format!("{}/{}/", emu_type, system_name);
        Object::list_prefix("oraksil-games", &prefix).unwrap()
            .into_iter()
            .filter(|r| r.name != prefix)
            .collect()
    }
}

fn gcp_download_url_to_path(url: &str, path: &Path) {
    let response = reqwest::blocking::get(url).unwrap();
    let content =  response.bytes().unwrap();

    let mut file = File::create(path).unwrap();
    file.write_all(&content).unwrap();
}

impl RomManager for GcpRomManager {
    fn pull_roms(&mut self, emu_type: &str, system_name: &str) -> Result<(), String> {
        let rom_objects = self.list_rom_objects(&emu_type, &system_name);
            
        self.download_objects(&rom_objects);

        println!("download done..");

        Ok(())
    }
}
