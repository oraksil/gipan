use std::path::Path;
use std::fs::File;
use std::io::Write;
use std::thread;

use reqwest;
use cloud_storage::Object;
use s3::creds::Credentials;
use s3::bucket::Bucket;

pub trait RomManager {
    fn pull_roms(&self, emu_type: &str, system_name: &str) -> Result<(), String>;
}

pub struct AwsRomManager {
    base_roms_path: String,
}

impl AwsRomManager {
    pub fn create(roms_path: &str) -> impl RomManager {
        AwsRomManager {
            base_roms_path: String::from(roms_path)
        }
    }

    fn credentials(&self) -> Credentials {
        Credentials::from_env_specific(
                Some("AWS_ACCESS_KEY_ID"),
                Some("AWS_SECRET_ACCESS_KEY"),
                None,
                None).unwrap()
    }

    fn download_objects(&self, objects: &Vec<String>) {
        for o in objects {
            println!("{}", o);
        }
        // let join_handles: Vec<std::thread::JoinHandle<_>> = objects.into_iter()
        //     .map(|o| { 
        //         println!("downloading... {}", o.name);

        //         let url = o.download_url(120).unwrap();
        //         let filename = Path::new(&o.name).file_name().unwrap();
        //         let path = Path::new(&self.base_roms_path).join(filename);
        //         thread::spawn(move || gcp_download_url_to_path(&url, &path))
        //     })
        //     .collect();

        // for h in join_handles {
        //     let _ = h.join();
        // }
    }

    fn list_rom_objects(&self, emu_type: &str, system_name: &str) -> Vec<String> {
        // let client = S3Client::new(Region::ApNortheast2);
        // let mut list_request = ListObjectsRequest::default();
        // list_request.bucket = "oraksil".to_string();
        // list_request.prefix = Some(format!("/games/{}/{}", emu_type, system_name));

        // let result = client.list_objects_v2(list_request)
        // println!("result is {:?}", result);


        let region = "ap-northeast-2".parse().unwrap();
        let bucket = Bucket::new("oraksil", region, self.credentials()).unwrap();
        let prefix = format!("/games/{}/{}/", emu_type, system_name);
        let results = bucket.list_blocking(prefix, None).unwrap();
        for (l, c) in results {
            println!("{:?}", l)
        }

        vec!()
    }
}

impl RomManager for AwsRomManager {
    fn pull_roms(&self, emu_type: &str, system_name: &str) -> Result<(), String> {
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
    fn pull_roms(&self, emu_type: &str, system_name: &str) -> Result<(), String> {
        let rom_objects = self.list_rom_objects(&emu_type, &system_name);
            
        self.download_objects(&rom_objects);

        println!("download done..");

        Ok(())
    }
}
