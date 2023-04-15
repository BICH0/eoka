use std::fs::{File};
use std::{fs,env};
use std::process::Command;
use std::time::Instant;
use std::cmp::min;
use std::io::{Write,Read};
// use std::process::exit;
use futures_util::StreamExt;
use reqwest::{Response,Error};
use rusqlite::{Connection,CachedStatement};
use indicatif::{ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use human_bytes::human_bytes;
use blake3;

lazy_static! {
    static ref LOCAL:SQLite = SQLite::new("eoka.db");
    static ref REMOTE:SQLite = SQLite::new("remote.db");
}

#[derive (Debug,Clone)]
struct Package{
    name: String,
    lversion: String,
    rversion: String,
    dependencies: Option<String>,
}

struct SQLite{
    path: String,
}
impl SQLite{
    pub fn new(path:&str) -> Self{
        Self {path:path.to_string()}
    }
    fn new_con(&self) -> Connection{
       return Connection::open(&self.path).expect("Failure while opening db");
    }
    fn list_pkg(&self, conn:Connection, pkgname:&str) -> Vec<Package>{
        let mut stmt = match conn.prepare("SELECT * FROM packages WHERE name like :id;"){
            Ok(query) => {
                query
            },
            Err(e) => {
                println!("The following error ocurred: {}",e);
                return Vec::new();
            }
        };
        let rows = stmt.query_map(&[(":id",pkgname)], |row| {
            Ok(Package {
                name: row.get(0).unwrap(),
                lversion: row.get(1).unwrap(),
                rversion: row.get(2).unwrap(),
                dependencies: row.get(3).unwrap(),
            })
        }).expect("Unable to process all the lines");
        let mut value = Vec::new();
        for row in rows {
            value.push(row.unwrap());
        }
        return value
    }
    fn update_pkg(&self, conn:Connection, pkgname:&str, nvalue:String, field:&str){
        let mut stmt:CachedStatement;
        if field == "rversion"{
            stmt = conn.prepare_cached("UPDATE packages SET rversion = ?1 WHERE name = ?2").expect("Unable to prepare the function");
        }else{
            stmt = conn.prepare_cached("UPDATE packages SET lversion = ?1 WHERE name = ?2").expect("Unable to prepare the function");
        }
        // println!("{},{}",nvalue,pkgname);
        stmt.execute([&nvalue,pkgname]).expect("Unable to execute the function");
    }
    fn add_pkg(&self, conn:Connection, name:&String, rversion:&String, depen:Option<String>){
        let mut stmt = conn.prepare("INSERT INTO packages VALUES(?1,'0',?2,?3)").expect("Unable to prepare the function");
        stmt.execute([name,rversion,&depen.unwrap()]);//TODO handle err
    }
    fn rem_pkg(&self, conn:Connection, name:&String){
        let mut stmt = conn.prepare_cached("DELETE FROM packages WHERE name = ?1").expect("Unable to prepare the function");
        stmt.execute([name]);//TODO handle err
    }
}
async fn user_input() -> bool{
    let mut user_input:String = String::new();
    while user_input != "y" && user_input != "n"{
        user_input.clear();
        std::io::stdin().read_line(&mut user_input).expect("Error reading");
        user_input = user_input.trim().to_lowercase();
        if user_input == ""{
            user_input = "y".to_string();
        }
    }
    if user_input == "y"{
        return true;
    }
    false
}
fn cmp_v (lv:&String, rv:&String) -> bool{//Revisar porque si hay paquetes 2.3.0-b o algo asi se lo carga
    let lv:Vec<&str> = lv.split(".").collect();
    let rv:Vec<&str> = rv.split(".").collect();
    let mut result:bool = false;
    for x in 0..lv.len(){
        if lv[x] > rv[x]{
            result=false;
            break;
        }else if lv[x] == rv[x]{
           
        }else{
            result=true;
            break;
        }
    }
    return result;
}
async fn check_fs (name:&str) -> Option<Response>{
    async fn check(server:&str, fname:&str)-> Result<Response,Error>{
        let src:String;
        if fname != "eoka.db"{
            let file:String = fname.split("-").next().expect("Empty value").to_string();
            src = format!("http://{}/{}/{}",server,file,fname);
        }else{
            src = format!("http://{}/{}",server,fname);
        }
        reqwest::get(&src).await
    }
    for server in ["mirror.nrtheriam.es","mirror.confugiradores.es"]{
        match check(server,&name).await{
            Ok(resp) => {
                if resp.status().as_str() == "404"{//TODO Formatear esto
                    println!("Oh no - File {} not found on this mirror", name);
                    continue;
                }else{
                    return Some(resp);
                }
            },
            Err(e) => {
                println!("Oh no - {e}");
            }
        }
    }
    None

}
async fn get_file(res:Response,fname:&str) -> File{
    let fs:u64 = res.content_length().expect("Error fetching file size");
    let pb = ProgressBar::new(fs);
    pb.set_style(ProgressStyle::default_bar()
        .template("{msg}\n{spinner:.red} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})").expect("Error theming the progress bar"));
    pb.set_message(format!("Downloading {}", fname));
    let mut file = File::create(fname).expect("Error while creating file");
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = item.expect("Error while downloading file");
        file.write_all(&chunk).expect("Error while writing to file");
        let new = min(downloaded + (chunk.len() as u64), fs);
        downloaded = new;
        pb.set_position(new);
    }
    pb.finish_with_message(format!("Downloaded {}", fname));
    return file
}

async fn option_sync(){
    println!("Syncing databases");
    let res:Option<Response> = check_fs("eoka.db").await;
    get_file(res.expect(""),&REMOTE.path).await;
    let packages = REMOTE.list_pkg(REMOTE.new_con(),"%");
    let pb = ProgressBar::new(packages.len().try_into().unwrap());
    pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{wide_bar:.red}] {pos}/{len} ({msg}, {eta})").unwrap());
    let mut n:u32 = 0;
    let mut u:u32 = 0;
    for pack in packages{
        let lpackage = LOCAL.list_pkg(LOCAL.new_con(), &pack.name);
        if lpackage.is_empty(){
            LOCAL.add_pkg(LOCAL.new_con(), &pack.name, &pack.rversion, pack.dependencies);
            n+=1;
        }else{
            if cmp_v(&lpackage[0].lversion, &pack.rversion){
                LOCAL.update_pkg(LOCAL.new_con(), &pack.name, pack.rversion, "rversion");
                u+=1;
            }
        }
        pb.set_message(pack.name);
        pb.inc(1);
    }
    pb.finish_with_message("Finished updating the dabase");
    fs::remove_file("remote.db");//TODO handle err
    println!("Update endeded with {} new packages, and {} updates available.",&n,&u);
}
async fn option_install(packages:Vec<String>){
    for package in packages{
        let pkgname = package.to_string();
        println!("Installing package {}", package);
        let mut pkg:Vec<&str> = {//Separamos nombre y version
            if pkgname.contains("="){
                pkgname.split("=").collect()
            }else{
                vec![&pkgname]
            }
        };
        //Cambiar este if para que haga el list si o si y que si no hay fallo haga la comprobacion
        let dbversion = LOCAL.list_pkg(LOCAL.new_con(),pkg[0]);
        match dbversion.first(){
            None => {println!("Unable to find package {}", pkg[0]);return;},
            val => {
                for dependency in &val.unwrap().dependencies{
                    //installpkg
                }
                if pkg.len() == 1{
                    pkg.push(&val.unwrap().rversion);
                }else{
                    //comprobar si la version que hay instalada es inferior a la ultima remota, si si, continuar si no, parar
                }
            }
        }
        let tgzname:&str = &format!("{}-{}.tgz",pkg[0],pkg[1]);
        let resp:Option<Response> = check_fs(tgzname).await;
        if resp.as_ref().expect("File doesnt exist").status().as_str() == "404"{//TODO Match
            println!("Oh no - File not found on this mirror");
        }else{
            let resp:Response = resp.expect("File doesnt exist");
            println!("Eoka will need to download {} to install {} continue? Y/n",human_bytes(resp.content_length().expect("Failed to retrieve fs") as f64),pkg[0]);
            if ! user_input().await{
                return;
            }
            get_file(resp,tgzname).await;
            if ! hash_check(tgzname).await {
                println!("File integrity check failed, aborting installation...");
                return;
            }
            let tempdir:&str = &format!("/tmp/build-{}",pkg[0]);
            fs::create_dir(tempdir).expect("");
            let output = Command::new("/bin/tar").args(["-xzf",tgzname,"-C",&tempdir]).output();
            println!("{:?}",output);
            //install
            
        }
    }
}
async fn hash_check (file:&str) -> bool{
    let mut bfile:Vec<&str> = file.split(".").collect();
    bfile.pop();
    bfile.push("b3");
    let bfile:&str = &bfile.join(".");
    let resp:Option<Response> = check_fs(bfile).await;
    if resp.expect("File doesnt exist").status().as_str() == "404"{//TODO Match
        println!("Oh no - File not found, package integrity could not be verified.");
        return false
    }
    let mut bytes:Vec<u8> = Vec::new();
    match File::open(file) {
        Ok(mut file) => {
            file.read_to_end(&mut bytes);
            let mut hasher = blake3::Hasher::new();
            let start = Instant::now();
            hasher.update(&bytes);
            let hash = hasher.finalize();
            println!("HASH: {} in {:?}", hash, start.elapsed());
            let mut rhash:String = String::new();
            File::open(bfile).expect("Unable to open file").read_to_string(&mut rhash);//TODO Handle err
            return hash.to_string().eq(rhash.split(" ").collect::<Vec<_>>()[0])
        },
        Err(_) => {
            println!("Unable to open file");
            false
        }

    }
}

/*Install
    
*/

#[tokio::main]
async fn main(){
    let mut args:Vec<String> = env::args().collect();
    for x in 1..args.len(){
        match args[x].as_str(){
            "-s" | "-S" | "update" =>{
                option_sync().await;
                return;
            },
            "upgrade" => {
                //EN FIN EL CHISTE SE CUENTA SOLO
                // if args[x+1].is_none(){
                //     for package in local.list_pkg(){
                        
                //     }
                // }
            },
            "-i" | "install" => {
                option_install(args.drain(x+1..).collect()).await;
                return;
            },
            "list" => {

            }
            "remove" => {

            }
            _ => println!("Argument {} unrecognised.",args[x])
        }
    }
    
}
