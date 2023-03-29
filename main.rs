use std::env;
use std::fs::{File, rename};
use std::io::{self, BufRead, copy, Cursor};
use std::path::Path;
use rusqlite::{Connection, Result};
use tempfile::Builder;
// use reqwest::get;
// use indicatif::*;
use version_compare::{Cmp, Version};
const VERSION:&str = "Eoka 0.0.0";

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where P: AsRef<Path>, {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}
fn open_file(file: &str){
   if let Ok(lines) = read_lines(file){
      for line in lines {
         if let Ok(ip) = line {
            println!("{}", ip);
         }
      }
   }
}
fn sql_get (database:&str, value:&str) -> Result<Vec<String>> {
    let localdb = Connection::open(database)?;
    let mut stmt = localdb.prepare("SELECT * FROM data WHERE name = :id")?;
    let rows = stmt.query_map(&[(":id", value)], |row| row.get(1))?;
    let mut names = Vec::new();
    for name_result in rows {
        names.push(name_result?);
    }
    Ok(names)
}
async fn download_file(url:&str,verbose:bool)->String{//Checkear donde quiero guardar las ddbb -> /etc/eoka.d/remote.db
    if verbose {
        println!("Starting download");
    }
    let target = "http://".to_owned() + url;
    let response = reqwest::get(target).await.expect("Error on the request");
    let fname = url.split("/").last().expect("Invalid URL").to_owned();
    let mut file = Cursor::new(response.bytes().await.expect("Invalid content"));
    let mut dest = File::create(&fname).expect("Could not create the file");
    copy(&mut file, &mut dest).expect("Error while copying the file");
    return fname;
}
async fn compare_versions(verbose:bool) -> bool{
    let local = sql_get("local.db","version").unwrap();
    let remote = download_file("mirror.confugiradores.es/eoka.db",verbose).await;
    let remote = sql_get(&remote,"version").unwrap();
    if verbose{
        println!("Local version: {}\nRemote version: {}", &local[0], &remote[0]);
    }
    let local = Version::from(&local[0]).unwrap();
    let remote = Version::from(&remote[0]).unwrap();
    match local.compare(remote){
        Cmp::Lt => return true,
        _ => return false,
    }
}
#[tokio::main]
async fn main() {
    let mut verbose:bool = true;//Invertir 
    let args = env::args().skip(1);
    if verbose{//DELETE
        println!("{:?}",args);
    }
    for arg in args{//Cambiar la estructura para poder saltar en caso de que fuese necesario
        if arg.chars().nth(0).unwrap() == '-'{//Cambiar para que se contemple --
            match arg.chars().nth(1).unwrap(){
                'V' => println!("{}",VERSION),
                'v' => verbose = true,
                'L' => {//Installed, upgradable, available
                    
                },
                'S' => {
                    
                },
                'i' => println!("Installing package"),
                'r' => println!("Removing package"),
                'p' => println!("Purging package"),
                'u' => println!("Updating package"),
                'U' => println!("Upgrading all packages"),
                'l' => println!("Eoka options\n-V: Get Eoka version\n-v: Enable verbosity\n-l: List all available commands"),
                _ => println!("Unknown argument, use eoka -l to list all available commands"),
            }
        }else{
            match arg.as_str(){
                "sync" => println!("Syncing database"),
                "install" => println!("Installing package"),
                "remove" => println!("Removing package"),
                "purge" => println!("Purging package"),
                "update" => println!("Updating package"),
                "upgrade" => println!("Upgrading all packages"),
                _ => println!("No luck"),
            }
        }
    }
    if verbose{
        println!("Verbose enabled");
    }
    let result = compare_versions(verbose).await;
    if result {
        println!("WAWAWAWA");
    }
    open_file("../repos.txt");
}
