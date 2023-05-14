use std::fs::{File,OpenOptions};
use std::{fs,env};
use std::process::Command;
use std::time::Instant;
use std::cmp::min;
use std::io::{Write,Read,BufRead};
use std::collections::HashMap;
// use std::process::exit;
use futures_util::StreamExt;
use reqwest::{Response,Error};
use rusqlite::{Connection,CachedStatement};
use indicatif::{ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use human_bytes::human_bytes;
use blake3;
use colored::{Colorize,ColoredString};

const EOKA_TMP:&str = "/tmp/";
const EOKA_LOCAL:&str = "/etc/eoka/";
const VERSION:&str = "0.0.2";
lazy_static! {
    static ref LOCAL:SQLite = SQLite::new("eoka.db",EOKA_LOCAL);//Cambiar segundo campo a eoka_local
    static ref REMOTE:SQLite = SQLite::new("eoka.db",EOKA_TMP);
    static ref FS:FileSystem = FileSystem::new();
}

#[derive (Debug,Clone)]
struct Package{
    name: String,
    lversion: String,
    rversion: String,
    deps: Option<String>,
}

struct SQLite{
    name: String,
    path: String,
}
impl SQLite{
    pub fn new(name:&str,path:&str) -> Self{
        Self {name:name.to_string(),path:path.to_string()+name}
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
                deps: {if row.get::<usize, String>(3).unwrap() == ""{
                    None
                }else{
                    Some(row.get(3).unwrap())
                }}
            })
        }).expect("Unable to process all the lines");
        let mut value = Vec::new();
        for row in rows {
            value.push(row.unwrap());
        }
        return value
    }
    fn update_pkg(&self, conn:Connection, pkgname:&str, nvalue:&String, field:&str, ndeps:&Option<String>){
        let mut stmt:CachedStatement;
        if field == "rversion"{
            stmt=conn.prepare_cached("UPDATE packages SET rversion = ?1, deps = ?2 WHERE name = ?3").expect("Unable to prepare the function");
            let ndeps = ndeps.clone().unwrap();
            stmt.execute([&nvalue,&ndeps.to_string(),&pkgname.to_string()]).expect("Unable to execute the function");
        }else{
            stmt=conn.prepare_cached("UPDATE packages SET lversion = ?1 WHERE name = ?2").expect("Unable to prepare the function");
            stmt.execute([&nvalue,&pkgname.to_string()]).expect("Unable to execute the function");
        }
    }
    fn add_pkg(&self, conn:Connection, name:&String, rversion:&String, deps:Option<String>){
        let mut stmt = conn.prepare("INSERT INTO packages VALUES(?1,'0',?2,?3)").expect("Unable to prepare the function");
        let deps:String = match deps{
            Some(deps) => deps,
            None => "".to_string(),
        };
        stmt.execute([name,rversion,&deps]).unwrap();
    }
    fn check_dep(&self, conn:Connection, dep:&str, pkg:&str) -> bool{
        let mut stmt = conn.prepare("SELECT name, deps FROM packages WHERE deps LIKE :like;").unwrap();
        let rows = stmt.query_map(&[(":like",&format!("%{}%",dep))],|row| {
            Ok(
                [row.get::<usize,String>(0).unwrap(),row.get::<usize,String>(1).unwrap()]
            )
        }).unwrap();
        for row in rows {
            let mut row=row.unwrap();
            println!("{:?} ? {}",row[0],pkg);
            if row[0] == pkg{
                continue
            }else{
                row[1]=row[1].replace(":",",");
                for dep in row[1].split(","){
                    if dep.split("-").nth(0).unwrap() == dep{
                        return true
                    }
                }
            }
        }
        return false;
    }
}
struct FileSystem;
impl FileSystem{
    fn new() -> Self{
        Self
    }
    fn create_dir(&self,dir:String) -> u32{
        match fs::create_dir(&dir) {
            Ok(_) => return 0,
            Err(e) => {
                if e.to_string().contains("os error 17") {
                    match fs::remove_dir_all(&dir){
                        Ok(_) => self.create_dir(dir),
                        Err(e) => {
                            println!("Oh no - {}",e);
                            return 1
                        },
                    }
                }else{
                    println!("An error ocurred while creating temporal dir -> {}", e);
                    return 1
                }
            },
        }
    }
    fn read_file(&self,path:String) -> Vec<String>{
        let file = match File::open(&path){
            Ok(e) => e,
            Err(_)=> {
                println!("Oh no - File {} doesn't exist!!",&path);
                return Vec::new()
            },
        };
        let reader = std::io::BufReader::new(file);
        let mut res = Vec::new();
        for line in reader.lines(){
            res.push(line.unwrap());
        }
        res
    }
}
fn print_exit(method:u8,pre:&str,end:bool){
    let value:ColoredString = match method{
        0 => "[OK]".green(),
        1 => "[ERROR]".red(),
        3 => "[WARN]".yellow(),
        _ => "[INFO]".purple(),
    };
    if end{
        println!("{}{}",pre,value);
    }else{
        print!("{}{} ",pre,value);
    }
}
fn user_input(def:&str) -> bool{
    let mut user_input:String = String::new();
    while user_input != "y" && user_input != "n"{
        user_input.clear();
        std::io::stdin().read_line(&mut user_input).expect("Error reading");
        user_input = user_input.trim().to_lowercase();
        if user_input == ""{
            user_input = def.to_string();
        }
    }
    if user_input == "y"{
        return true;
    }
    false
}
fn cmp_v(lv:&String, rv:&String) -> bool{//Revisar porque si hay paquetes 2.3.0-b o algo asi se lo carga
    let mut lv:Vec<&str> = lv.split(".").collect();
    let rv:Vec<&str> = rv.split(".").collect();
    let mut result:bool = false;
    for x in 0..rv.len(){
        if lv.len() <= x{
            lv.push("0");
        }
    }
    for x in 0..lv.len(){
        if rv.len() <= x{
            result=false;
            break;
        }
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
async fn fetch_mirrors() -> Vec<String>{
    println!("Updating mirror list...");
    let mut mirrors = Vec::new();
    for line in FS.read_file(EOKA_LOCAL.to_string()+"sources.list"){
        match reqwest::get(format!("http://{}/eoka.db",&line)).await {
            Err(_) => {
                print_exit(1,"  ",false);
                println!("-> Mirror {} invalid or offline.", &line)
            },
            Ok(_) => {
                print_exit(0,"  ",false);
                println!("{}", line);
                mirrors.push(line);
            }
        }
    }
    println!("");
    mirrors
}
async fn check_fs(name:&str,mirrors:&mut Vec<String>) -> Option<Response>{
    async fn check(server:&str, fname:&str)-> Result<Response,Error>{
        let src:String = {
            if fname != "eoka.db" && fname != "eoka.b3"{
                let file:String = fname.split("-").next().expect("Empty value").to_string();
                format!("http://{}/{}/{}",server,file,fname)
            }else{
                format!("http://{}/{}",server,fname)
            }
        };
        reqwest::get(&src).await
    }
    for server in mirrors{
        match check(server.as_str(),&name).await{
            Ok(resp) => {
                if resp.status().as_str() == "404"{
                    print_exit(1,"\n  ",false);
                    println!("File {} not found on {}", &name, &server);
                    continue;
                }else{
                    return Some(resp);
                }
            },
            Err(e) => {
                //TODO Comprobar como sacar un mirror de aqui
                // let index = mirrors.iter().position(|y| &*y == server).unwrap();
                // mirrors.remove(index);
                if e.is_timeout(){
                    print_exit(1,"  ",false);
                    println!("Timeout");
                }else if e.is_connect(){
                    print_exit(1,"  ",false);
                    println!("Unable to connect with {}", {&server});
                }else{
                    print_exit(1,"  ",false);
                    println!("Error: {}", e.status().expect("0"));
                }
            }
        }
    }
    print_exit(1,"",false);

    None

}
async fn get_file(res:Response,fname:&str) -> String{
    let fs:u64 = res.content_length().expect("Error fetching file size");
    let pb = ProgressBar::new(fs);
    pb.set_style(ProgressStyle::default_bar()
        .progress_chars("=> ")
        .template("{msg}\n{spinner:.red} [{elapsed_precise}] [{bar:40.cyan}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})").expect("Error theming the progress bar"));
    pb.set_message(format!("Downloading {}", fname));
    let fpath:&str = &format!("{}{}",EOKA_TMP,fname);
    let mut file = File::create(fpath).expect("Error while creating file");
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
    return fpath.to_owned()
}

async fn hash_check(file:&str,mirrors:&mut Vec<String>) -> bool{
    let mut bfile:Vec<&str> = file.split(".").collect();
    bfile.pop();
    bfile.push("b3");
    let mut bfile:String = bfile.join(".");
    let resp:Response = check_fs(&bfile,mirrors).await.expect("File doesnt exist");
    if resp.status().as_str() == "404"{
        println!("Oh no - File not found, package integrity could not be verified.");
        return false
    }else{
        bfile=get_file(resp,&bfile).await;
    }
    let mut bytes:Vec<u8> = Vec::new();
    let file = &(EOKA_TMP.to_owned() + file);
    match File::open(file) {
        Ok(mut file) => {
            file.read_to_end(&mut bytes);
            let mut hasher = blake3::Hasher::new();
            let start = Instant::now();
            hasher.update(&bytes);
            let hash = hasher.finalize();
            println!("HASH: {} in {:?}", hash, start.elapsed());
            let mut rhash:String = String::new();
            File::open(bfile).expect("Unable to open checksum file").read_to_string(&mut rhash);//TODO Handle err
            return hash.to_string().eq(rhash.split(" ").collect::<Vec<_>>()[0])
        },
        Err(_) => {
            println!("Unable to open file");
            false
        }

    }
}
async fn unpack(pkgname:&str, pkgversion:&str, path:String, mirrors:&mut Vec<String>,upgrade:bool,oldv:&Option<String>){
    fn clean_pkg(installed:bool,tempdir:&str,pkg:&str){
        if ! installed{
            std::fs::remove_file(format!("{}bps/{}",EOKA_LOCAL,&pkg));
        }
        std::fs::remove_dir_all(&tempdir);
        std::fs::remove_file(format!("{}{}.tgz",EOKA_TMP,&pkg));
        std::fs::remove_file(format!("{}{}.b3",EOKA_TMP,&pkg));
        print_exit(0,"",true);
    }
    let pkg:String = format!("{}-{}",pkgname,pkgversion);
    if ! hash_check(format!("{}.tgz",&pkg).as_str(),mirrors).await {
        println!("File integrity check failed, aborting installation...");
        return;
    }else{
        println!("File integrity passed");
    }
    let tempdir:&str = &format!("/tmp/build-{}",pkgname);
    FS.create_dir(tempdir.to_string());
    let mut output = Command::new("/bin/tar").args(["-xzf",&path,"-C",&tempdir]).output().expect("Failed to execute");
    if output.status.success(){
        if ! std::path::Path::new(&(tempdir.to_owned()+"/blueprint")).exists() || ! std::path::Path::new(&(tempdir.to_owned()+"/blueprint")).exists(){
            println!("Package has been wrongly packed");
            return
        }
        for pfile in ["blueprint","conf.sh","data.tgz","post.sh"]{
            let fname:String =tempdir.to_owned()+"/"+pfile;
            match File::open(&fname){
                Ok(_) => {
                    match pfile{
                        "blueprint" => {
                            print!("Saving blueprint...");
                            match &oldv{
                                Some(oldv) => {
                                    fs::remove_file(format!("{}bps/{}-{}",EOKA_LOCAL,pkgname,oldv));
                                },
                                None => (),
                            }
                            match fs::copy(&fname, format!("{}bps/{}-{}",EOKA_LOCAL,pkgname,pkgversion)){
                                Ok(_) => print_exit(0,"",true),
                                Err(e) => {
                                    print_exit(1,"",false);
                                    println!("-> {}", e)
                                },
                            }
                            // let mut cmd = Command::new("/bin/tar").args(["-czf","blueprint","-C",&format!("{}.tgz",&pkg)]).spawn().expect("Unable to execute");//TODO cambiar a mv
                            // cmd.wait();

                        }
                        "conf.sh"|"post.sh" => {
                            print!("Executing {}...",&pfile);
                            let method:&str = if upgrade{
                                "update"
                            }else{
                                "install"
                            };
                            let mut cmd = Command::new("/bin/sh").args([&fname,method]).spawn().expect("Unable to execute");
                            if ! cmd.wait().unwrap().success() && pfile == "conf.sh"{
                                print_exit(1,"",true);
                                clean_pkg(false,tempdir,&pkg);
                                return
                            }
                            println!("[OK]");
                        },
                        "data.tgz" => {
                            print!("Unpacking source...");
                            output = Command::new("/bin/tar").args(["-xzf",&fname,"-C","/"]).output().expect("Failed to execute");
                            if ! output.status.success(){
                                print_exit(1," ",false);
                                println!("An error ocurred while extracting data \n{}", String::from_utf8(output.stderr).unwrap());
                                println!("{}",format!("Package {}-{} could not be installed",&pkgname, &pkgversion).red());
                                print!("Removing temportal files and aborting...");
                                clean_pkg(false,tempdir,&pkg);
                                return
                            }
                            print_exit(0,"",true);
                        },
                        e => {
                            print_exit(2,"",false);
                            println!("File {} not expected, skipping...",&e);
                        }
                    }
                },
                Err(e) => {
                    if e.raw_os_error() != Some(2) {
                        println!("Unable to open file {} because {:?}",&pfile,&e)
                    }
                },
            }
            
        }
    }else{
        println!("An error ocurred while extracting package {:?}", String::from_utf8(output.stderr).unwrap());
    }
    if upgrade {
        let file = OpenOptions::new()
            .append(true)
            .open(EOKA_LOCAL.to_owned()+"lastupdate.log");
        match file{
            Ok(mut file) =>{
                /*TODO cambiar la forma de obtener lversion
                    añadir fecha?
                */
                writeln!(file,"{}",format!("{} {} -> {}",pkgname,LOCAL.list_pkg(LOCAL.new_con(),&pkgname)[0].lversion,pkgversion)).unwrap();
            }
            Err(e) =>{
                println!("Unable to open the log file -> {}",e);
            }
        }

    }
    LOCAL.update_pkg(LOCAL.new_con(), &pkgname.to_string(), &pkgversion.to_string(), "lversion",&None);
    println!("{}",format!("Package {}-{} has been installed correctly",&pkgname, &pkgversion).green());
    print!("Cleaning enviorment...");
    clean_pkg(true,tempdir,&pkg);//TODO saber que coño contiene cada uno porque necesito muchas cosas
}
async fn reqwest_dependencies(bpdeps:&String,mirrors:&mut Vec<String>) -> Option<Vec<Vec<reqwest::Response>>>{
    let mut result:Vec<Vec<Response>>=Vec::new();
    let bpdeps:Vec<&str>=bpdeps.split(":").collect();
    let mut deps:Vec<Response> = Vec::new();
    for dep in bpdeps[0].split(","){//TODO checkear si dep contiene la version
        if dep == ""{
            continue;
        }
        match check_fs(format!("{}-{}.tgz",dep,LOCAL.list_pkg(LOCAL.new_con(),dep).first().unwrap().rversion).as_str(),mirrors).await{
            None => {
                print_exit(1,"     ",false);
                println!("Unable to fetch {:?}", dep);
                return None;
            }
            res => deps.push(res.unwrap()),
        }
    }
    result.push(deps);
    let mut opts:Vec<Response> = Vec::new();
    for opt in bpdeps[1].split(","){
        if opt == ""{
            continue;
        }
        match check_fs(opt,mirrors).await{
            None => {
                print_exit(1,"     ",false);
                println!("Unable to fetch {}", opt);
                return None;
            }
            res => opts.push(res.unwrap()),
        }
    }
    result.push(opts);//TODO optimizar esto
    Some(result)
}
async fn package_install(package:String,mirrors:&mut Vec<String>,mut upgrade:bool){
    let pkgname = package.to_string();
    let mut oldv:Option<String> = None;
    let mut pkg:Vec<&str> = {//TODO mover esto a otro sitio o dejarlo
        if pkgname.contains("="){
            pkgname.split("=").collect()
        }else{
            vec![&pkgname]
        }
    };
    let dbversion = LOCAL.list_pkg(LOCAL.new_con(),pkg[0]);
    let dbversion = match dbversion.first(){
        Some(dbv) => dbv,
        None => {
            print_exit(1,"",false);
            println!("Unable to find package {} try again after eoka sync",pkg[0]);
            return
        }
    };
    if &dbversion.name == ""{
        print_exit(1,"",false);
        println!("  Unable to find package {} try again after eoka sync", pkg[0]);
        return;
    }else{
        if pkg.len() == 1{
            pkg.push(&dbversion.rversion);
        }
    }
    if &dbversion.lversion == pkg[1]{
        println!("The package {} is already in the latest version {}",&dbversion.name,&dbversion.lversion);
        println!("Do you want to reinstall it? y/N");
        if ! user_input("n"){
            return;
        } 
    }else if &dbversion.lversion != "0"{
        println!("The package {} is installed with version {} and you are trying to install {}",&dbversion.name,&dbversion.lversion,pkg[1]);
        println!("Do you want to upgrade/downgrade it? y/N");//TODO dar opcion para instarlo independientemente
        if ! user_input("n"){
            return;
        }
        upgrade=true;
        oldv=Some(dbversion.lversion.clone());
    }
    if upgrade {
        println!("Upgrading package {}", pkg[0]);
        oldv=Some(dbversion.lversion.clone());
    }else{
        println!("Installing package {}", pkg[0]);
    }
    let bpdeps = dbversion.deps.clone();//Cojo las dependencias de la base de datos
    let deps:Option<Vec<Vec<reqwest::Response>>> = {
        match bpdeps{
            Some(bpdeps) => {
                print!(" - Fetching dependencies...");
                let res = reqwest_dependencies(&bpdeps,mirrors).await;
                print_exit(0,"",true);
                res
            },
            None => None,
        }
    };
    print!(" - Fetching package...");
    let tgzname:&str = &format!("{}-{}.tgz",pkg[0],pkg[1]);
    let resp:Response = match check_fs(tgzname,mirrors).await{
        Some(resp) => {
            if resp.status().as_str() == "404"{
                print_exit(1,"",true);
                println!("File not found on this mirror");
                return 
            }
            print_exit(0,"",true);
            resp
        },
        None => {
            println!("{}",format!("Package {} with version {} not found",pkg[0],pkg[1]).red());
            return
        }
    };
    let fops:Vec<Response>;
    let mut fdeps:Vec<Response> = Vec::new();
    match deps{//TODO dejarlo bonito
        Some(mut e) => {
            let dsize:u64 = {
                if e.len() != 0{
                    let mut size:u64 = 0;
                    for dep in &e[0]{
                        size = size + &dep.content_length().unwrap();
                    }
                    size
                }else{
                    0 
                }
            };
            let osize:u64 = {
                if e.len() != 0{
                    let mut size:u64 = 0;
                    for opt in &e[1]{
                        size = size + &opt.content_length().unwrap();
                    }
                    size
                }else{
                    0
                }
            };
            fops = match e.pop(){
                None => Vec::new(),
                Some(f) => f,
            };
            fdeps = match e.pop(){
                None => Vec::new(),
                Some(f) => f,
            };
            println!("The package {} has {} dependecies ({}) and {} optional dependencies ({})",pkg[0], fdeps.len(), human_bytes(dsize as f64), fops.len(), human_bytes(osize as f64));
            drop(e);
        },
        None => {
            if ! &dbversion.deps.is_none(){
                print_exit(1,"",false);
                println!("Failure while obtaining package dependencies, cannot install the package.");
                return
            }
        },
    }
    println!("Eoka will need to download {} to install {} continue? Y/n",human_bytes(resp.content_length().expect("Failed to retrieve fs") as f64),pkg[0]);
    if ! user_input("y"){
        return;
    }
    if fdeps.len() != 0 {
        for dep in fdeps{
            let dep_tmp = dep.url().path().split("/").last().unwrap().split("-");
            let dep_v:String = {
                let temp = dep_tmp.clone().last().unwrap().to_string();
                let mut temp:Vec<&str> = temp.split(".").collect();
                temp.pop();
                temp.join(".").to_string()
            };
            let dep_n:String = dep_tmp.clone().nth(0).unwrap().to_string();
            // println!("Instalando {:?} {:?}", dep_n, dep_v);
            unpack(&dep_n,&dep_v,get_file(dep,&format!("{}-{}.tgz",dep_n,dep_v)).await, mirrors, upgrade,&oldv).await;
        }
    }
    unpack(pkg[0],pkg[1], get_file(resp,tgzname).await, mirrors,upgrade,&oldv).await;
}
fn package_remove(package:String,fetch:bool){
    let mut package:Vec<&str> = package.split("=").collect();
    let dbpkg:Package = match LOCAL.list_pkg(LOCAL.new_con(),package[0]).first(){
        Some(pkgdata) => {
            if pkgdata.lversion == "0"{
                println!("Package {} is not installed", package[0]);
                return
            }
            pkgdata.clone()
        },
        None => {
            println!("Package {} not found.",package[0]);
            return
        }
    };
    println!("Removing package {}",&package[0]);
    if package.len() == 1{
        package.push(&dbpkg.lversion);
    }
    let lines:Vec<String> = FS.read_file(EOKA_LOCAL.to_owned() + "bps/" + &package[0] + "-" + &package[1]);
    if lines.len() == 0{
        print_exit(1,"",false);
        if package[1] != &dbpkg.lversion{
            println!("Package {} is installed with version {}\nTry again with eoka remove {}={} or eoka remove {}",&package[0],&dbpkg.lversion,&package[0],&dbpkg.lversion,&package[0])
        }else{
            println!("Package {} is installed but no blueprint exist or is corrupted ({})", &package[0],format!("{}bps/{}-{}",EOKA_LOCAL.to_owned(),&package[0],&package[1]));
        }
        return
    }
    println!("Uninstalling {}-{}",&package[0],&package[1]);

    fn iter(pkgname:&str,lines:Vec<String>,fetch:bool) -> Option<Vec<String>>{
        for line in lines{
            let line:Vec<&str> = line.split(":").collect();
            match line[0]{
                "" => {
                    println!("Unknown line");
                },
                field =>{
                    match field{
                        "Dependencies" => {
                            if fetch{
                                if line[1] == "" && line[3] == ""{
                                    break;
                                }
                                println!("Package has some dependencies, do you want to uninstall them whenever is possible (no package has depency on it) Y/n");
                                if user_input("y") {
                                    let mut deplist:Vec<String> = Vec::new();
                                    match line[1]{
                                        "" => (),
                                        deps => {
                                            for dep in deps.split(","){
                                                if ! LOCAL.check_dep(LOCAL.new_con(),dep,pkgname){
                                                    println!("DEP {}",dep);
                                                    deplist.push(dep.to_string());
                                                }
                                            }
                                        }
                                    }
                                    match line[3]{
                                        "" => (),
                                        opts => {
                                            for opt in opts.split(","){
                                                if ! LOCAL.check_dep(LOCAL.new_con(),opt,pkgname){
                                                    println!("DEP {}",opt);
                                                    deplist.push(opt.to_string());
                                                }
                                            }
                                        }
                                    }
                                    if deplist.len() != 0 {
                                        return Some(deplist)
                                    }
                                }
                            }
                        },
                        "Paths" => {
                            match line[1]{
                                "" => {
                                    println!("Blueprint is malformed, could not uninstall");
                                    break;
                                },
                                paths => {
                                    println!("Removing files...");
                                    let paths:Vec<&str> = paths.split(",").collect();
                                    let pb = ProgressBar::new(paths.len().try_into().unwrap());
                                    pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{bar:40.cyan}] {pos}/{len} ({msg}, {eta})").unwrap()
                                        .progress_chars("=> ")
                                    );
                                    for path in paths{
                                        match fs::remove_dir_all(&path){
                                            Err(e) => {
                                                if e.to_string() == "Not a directory (os error 20)"{
                                                    match fs::remove_file(&path){
                                                        Err(e) => {
                                                            println!("This path could not be removed, aborting due to {}",e);
                                                            return Some(Vec::new());
                                                        },
                                                        Ok(_) => (),
                                                    }
                                                }else{
                                                    println!("This path could not be removed, aborting due to {}",e);
                                                    return Some(Vec::new());
                                                }
                                            },
                                            Ok(_) =>{
                                                pb.inc(1);
                                            }
                                        }
                                    }
                                    pb.finish_with_message("Finished removing files");
                                }
                            }
                        },
                        uk => println!("Unknwon line {}",uk),
                    }
                },
                
            }
        }
        return None
    }
    match iter(&package[0],lines,fetch){
        Some(deps) => {
            let pb = ProgressBar::new(deps.len().try_into().unwrap());
            pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{bar:40.cyan}] {pos}/{len} ({msg}, {eta})").unwrap()
                .progress_chars("=> ")
            );
            for dep in deps{
                pb.set_message(dep.clone());
                package_remove(dep,true);
                pb.inc(1);
            }
            pb.finish_with_message("Finished removing files");
            package_remove(package[0].to_string(),false);
        },
        None => {
            println!("{}",format!("Package {} has been uninstalled",&package[0]).green());
        }
    }
    print!("Flushing database ");
    LOCAL.update_pkg(LOCAL.new_con(), &package[0], &"0".to_string(), "lversion", &dbpkg.deps);
    print_exit(0,"",true);
    // print!("Cleaning system ");
    match fs::remove_file(format!("{}bps/{}-{}",EOKA_LOCAL,package[0],package[1])){
        Ok(_) => print_exit(0,"",true),
        Err(e) => {
            print_exit(1,"",true);
            println!("{}",e);
        },
    }
    return
}
#[tokio::main]
async fn main(){
    let mut args:Vec<String> = env::args().collect();
    for x in 1..args.len(){
        match args[x].as_str(){
            "-s" | "-S" | "update" | "sync" =>{
                let mut mirrors:Vec<String> = fetch_mirrors().await;
                println!("Syncing the database");
                let res:Option<Response> = check_fs("eoka.db",&mut mirrors).await;
                get_file(res.expect(""),&REMOTE.name).await;

                if ! hash_check("eoka.db",&mut mirrors).await {
                    println!("File integrity check failed, aborting update...");
                    return;
                }else{
                    println!("File integrity passed");
                }
                let packages = REMOTE.list_pkg(REMOTE.new_con(),"%");
                let pb = ProgressBar::new(packages.len().try_into().unwrap());
                pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{bar:40.cyan}] {pos}/{len} ({msg}, {eta})").unwrap()
                    .progress_chars("=> ")
                );
                let mut n:u32 = 0;
                let mut u:u32 = 0;
                let mut lpackages:HashMap<String,Package>=HashMap::new();
                for pkg in LOCAL.list_pkg(LOCAL.new_con(), "%"){
                    lpackages.insert(pkg.name.clone(),pkg);
                }
                for pack in packages{
                    if ! lpackages.contains_key(&pack.name){
                        LOCAL.add_pkg(LOCAL.new_con(), &pack.name, &pack.rversion, pack.deps);
                        n+=1;
                    }else{
                        let lpackage = &lpackages[&pack.name];
                        //TODO notifico cuando hay una actualizacion en la bbdd o solo cuando están instalados
                        if cmp_v(&lpackage.rversion, &pack.rversion){
                            LOCAL.update_pkg(LOCAL.new_con(), &pack.name, &pack.rversion, "rversion", &pack.deps);
                            u+=1;
                        }
                    }
                    pb.set_message(pack.name);
                    pb.inc(1);
                }
                pb.finish_with_message("Finished updating the dabase");
                fs::remove_file(&REMOTE.path);//TODO handle err
                println!("Update endeded with {} new packages, and {} updates available.",&n,&u);
                return
            },
            "upgrade" => {
                let mut mirrors:Vec<String> = fetch_mirrors().await;
                if args.len() == x+1{
                    let mut up = 0;
                    for package in LOCAL.list_pkg(LOCAL.new_con(),"%"){
                        if package.lversion == "0" || package.lversion == package.rversion {
                            continue;
                        }
                        package_install(package.name,&mut mirrors,true).await;
                        up+=1;
                    }
                    println!("{} packages upgraded",up);
                }else{
                    let packages:Vec<String>=args.drain(x+1..).collect();
                    for pak in packages{
                        package_install(pak,&mut mirrors,true).await;
                    }
                    return
                }
            },
            "-i" | "install" => {
                let packages:Vec<String>=args.drain(x+1..).collect();
                let mut mirrors:Vec<String> = fetch_mirrors().await;
                for package in packages{
                    package_install(package,&mut mirrors,false).await
                }
                return
            },
            "list" => {
                if args.len() == x+1{
                    args.push("all".to_string());
                }
                let mut tot = 0;
                fn print_deps(pdeps:&Option<String>){
                    match pdeps{
                        Some(deps) =>{
                            println!("\n - Dependencies:");
                            let depes:Vec<&str> = deps.split(":").collect();
                            for dep in depes[0].split(","){
                                println!("   + {}", dep);
                            }
                            for opt in depes[1].split(","){
                                if opt != ""{
                                    println!("   + {} (Optional)", opt);
                                }
                            }
                        },
                        None => println!(""),
                    }
                }
                println!("Listing {} packages:",args[x+1]);
                match args[x+1].as_str(){
                    "upgradable" =>{
                        //Por cada paquete mostrar version instalada y la nueva, con un total de archivos.
                        for package in LOCAL.list_pkg(LOCAL.new_con(),"%"){
                            if &package.lversion == &"0"{
                                continue
                            }
                            if cmp_v(&package.lversion, &package.rversion){
                                tot+=1;
                                println!();
                                println!("{}",format!("Package {} -----------", &package.name).cyan());
                                print!(" - Local version: {} => Available version: {}", &package.lversion, &package.rversion);
                                print_deps(&package.deps);
                            }
                        }
                    },
                    "installed" =>{
                        println!();
                        for package in LOCAL.list_pkg(LOCAL.new_con(),"%"){
                            if package.lversion != "0" {
                                tot+=1;
                                print!("{}",format!("{} ({}) ", &package.name, &package.lversion).cyan());
                                print_deps(&package.deps);
                            }
                        }
                        //Por cada paquete mostrar nombre, version y dependencias
                    },
                    "updated" =>{
                        for line in FS.read_file(EOKA_LOCAL.to_owned()+"lastupdate.log"){
                            println!("{}",line);
                        }
                        //Mostrar los ultimos paquetes actualizados antiguo -> nuevo
                    },
                    "all" =>{
                        for package in LOCAL.list_pkg(LOCAL.new_con(),"%"){
                            println!("Package {}", package.name);
                        }
                        //Mostrar todos los paquetes
                    }
                    e => {//TODO mostrar informacion de paquete
                        println!("error: Option {} does not exist, use eoka list -h to list available options",e);
                    }
                }
                println!("\n---- Total packages {}",tot);
                return

            }
            "remove" => {
                let packages:Vec<String>=args.drain(x+1..).collect();
                if packages.len() != 0{
                    for package in packages{
                        package_remove(package,true);
                    }
                }else{
                    println!("You have to specify a package to install -> eoka install <pkgname>[=<version>]")
                }
                return
            }
            "-v"|"--version" =>{
                println!("Eoka {}",VERSION)
            }
            _ => println!("Argument {} unrecognised.",args[x])
        }
    }
    
}
