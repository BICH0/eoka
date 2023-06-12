use std::fs::{File,OpenOptions};
use std::{fs,env};
use std::process::Command;
use std::time::Instant;
use std::cmp::min;
use std::io::{Write,Read,BufRead};
use std::collections::HashMap;
use futures_util::StreamExt;
use reqwest::{Response,Error};
use rusqlite::{Connection,CachedStatement};
use indicatif::{ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use human_bytes::human_bytes;
use blake3;
use colored::{Colorize,ColoredString};
#[link(name = "c")]
extern "C" {
    fn geteuid() -> u32;
}

const EOKA_TMP:&str = "/tmp/";
const EOKA_LOCAL:&str = "/etc/eoka/";
const VERSION:&str = "1.0.0";
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
            let ndeps = match ndeps.clone(){
                Some(deps) => deps,
                None => "".to_string()
            };
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
        if user_input.is_empty(){
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
    let mut master = false;
    let mut m:u16=0;
    for line in FS.read_file(EOKA_LOCAL.to_string()+"sources.list"){
        match line.chars().nth(0).unwrap() {
            '#' => {
                match line.to_uppercase().as_str(){
                    "#MASTERS" => master=true,
                    "#SLAVES" => master=false,
                    _ => (),
                }
            },
            _ => {
                match reqwest::get(format!("http://{}/eoka.db",&line)).await {
                    Err(_) => {
                        print_exit(1,"  ",false);
                        println!("-> Mirror {} invalid or offline.", &line)
                    },
                    Ok(_) => {
                        let mut line = line;
                        print_exit(0,"  ",false);
                        println!("{}", line);
                        if master {
                            m+=1;
                            line=format!("^{}",line);
                        }
                        mirrors.push(line);
                    }
                }
            }
        }
    }
    if m < 1{
        print_exit(3,"",false);
        println!("No master servers available, file integrity can't be ensured");
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
    let mut filtered:Vec<String> = Vec::new();
    for server in &mut *mirrors{
        let server:String = {
            if server.chars().nth(0) == Some('^'){
                server.clone().split_off(1)
            }else{
                server.to_string()
            }
        };
        match check(server.as_str(),&name).await{
            Ok(resp) => {
                filtered.push(server.clone());
                if resp.status().as_str() == "404"{
                    print_exit(1,"\n  ",false);
                    println!("File {} not found on {}", &name, &server);
                    continue;
                }else{
                    return Some(resp);
                }
            },
            Err(e) => {
                if e.is_timeout(){
                    print_exit(1,"  ",false);
                    println!("Timeout");
                }else if e.is_connect(){
                    print_exit(1,"  ",false);
                    println!("Unable to connect with {}", &server);
                }else{
                    print_exit(1,"  ",false);
                    println!("Error: {}", e.status().expect("0"));
                }
            }
        }
    }
    *mirrors=filtered;
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
    let mut masters:Vec<String> = Vec::new();
    for master in mirrors.iter().take_while(|x|x.chars().nth(0)==Some('^')){
        masters.push(master.to_string());
    }
    let servers:&mut Vec<String> = {
        if masters.len() >= 1{
            &mut masters
        }else{
            println!("{}","No masters available, file integrity can't be trusted. Continue? y/N".yellow());
            if ! user_input("n"){
                return false
            }
            mirrors
        }
    };
    let resp:Response = check_fs(&bfile,servers).await.expect("File doesnt exist");
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
            match file.read_to_end(&mut bytes){
                Ok(_) => (),
                Err(_) => {
                    print!("Unable hash file ");
                    print_exit(1,"",true);
                    return false
                }
            }
            let mut hasher = blake3::Hasher::new();
            let start = Instant::now();
            hasher.update(&bytes);
            let hash = hasher.finalize();
            println!("HASH: {} in {:?}", hash, start.elapsed());
            let mut rhash:String = String::new();
            match File::open(bfile){
                Ok(mut res) => {
                    match res.read_to_string(&mut rhash){
                        Ok(_) => (),
                        Err(_) => {
                            print!("Unable to read hash file ");
                            print_exit(1,"",true);
                            return false
                        }
                    }
                    return hash.to_string().eq(rhash.split(" ").collect::<Vec<_>>()[0])
                },
                Err(e) => {
                    println!("Unable to open hash file due to -> {}",e);
                    return false
                }
            }
        },
        Err(_) => {
            println!("Unable to open file");
            false
        }

    }
}
async fn unpack(pkgname:&str, pkgversion:&str, path:String, mirrors:&mut Vec<String>,upgrade:bool,oldv:&Option<String>) -> bool{
    fn clean_pkg(installed:bool,tempdir:&str,pkg:&str){
        let mut failures:u8 = 0;
        fn rm_file(file:String,failures:&mut u8){
            let file:&str = file.as_str();
            match std::fs::remove_file(file){
                Ok(_)=>(),
                Err(_) => {
                    *failures+=1;
                    println!("\n Unable to remove {}",&file);
                    print_exit(1,"",true);
                }
            }
        }
        if ! installed{
            rm_file(format!("{}bps/{}",EOKA_LOCAL,&pkg),&mut failures);
        }
        match std::fs::remove_dir_all(&tempdir){
            Ok(_)=>(),
            Err(_) => {
                failures+=1;
                println!("\n Unable to remove blueprint");
                print_exit(1,"",true);
            }
        }
        rm_file(format!("{}{}.ek",EOKA_TMP,&pkg),&mut failures);
        rm_file(format!("{}{}.b3",EOKA_TMP,&pkg),&mut failures);
        if failures == 0{
            print_exit(0,"",true);
        }
        if ! installed{
            println!("{}","Package could not be installed due to above errors".red().bold());
            return;
        }
    }
    let pkg:String = format!("{}-{}",pkgname,pkgversion);
    if ! hash_check(format!("{}.ek",&pkg).as_str(),mirrors).await {
        print!("File integrity check failed, aborting installation...");
        print_exit(1,"",true);
        return false;
    }else{
        print!("File integrity passed ");
        print_exit(0,"",true);
    }
    let tempdir:&str = &format!("/tmp/build-{}",pkgname);
    FS.create_dir(tempdir.to_string());
    let mut output = Command::new("/bin/tar").args(["-xzhf",&path,"-C",&tempdir]).output().expect("Failed to execute");
    if output.status.success(){
        if ! std::path::Path::new(&(tempdir.to_owned()+"/blueprint")).exists() || ! std::path::Path::new(&(tempdir.to_owned()+"/blueprint")).exists(){
            println!("Package has been wrongly packed");
            return false;
        }
        let mut compiled:bool = true;
        let mut conf_stdout:Option<&str> = None;
        for pfile in ["blueprint","conf.sh","data.tgz","post.sh"]{
            let fname:String = tempdir.to_owned()+"/"+pfile;
            match File::open(&fname){
                Ok(_) => {
                    match pfile{
                        "blueprint" => {
                            print!("Fetching mode ");
                            let lines:Vec<String> = FS.read_file(fname.clone());
                            fn verify_mode(path:&String) -> bool{
                                println!("  Data.tgz root content");
                                let mut tmp_res:bool = false;
                                let cmd = Command::new("/bin/tar").args(["-tzf",&path]).output().expect("Unable to execute");
                                for line in std::str::from_utf8(&cmd.stdout).unwrap().split("\n").take_while(|x| x.matches("/").count() == 1 && x.chars().last() == Some('/')){
                                    match line{
                                        "var/"|"bin/"|"opt/"|"usr/"|"home/"|"etc/" => tmp_res=true,
                                        _ => (),
                                    }
                                    println!("{}",format!("    -{}",line).cyan());
                                }
                                if tmp_res{
                                    println!("  Package content seems to be compiled");
                                }else{
                                    println!("  Package content seems to be source")
                                }
                                println!("  Is it correct? Y/n");
                                if user_input("y"){
                                    return tmp_res
                                }
                                return !tmp_res
                            }
                            let datapath:String = tempdir.to_owned()+"/data.tgz";
                            {
                                if lines.len() >= 3{
                                    println!("MAY CONTAIN MODE {:?}",lines);
                                    let split:Vec<&str> = lines[2].split(":").collect();
                                    if split[0] == "Mode"{
                                        if split[1] == ""{
                                            let line:String = split[1].to_lowercase();
                                            match line.as_str(){
                                                "source" => {
                                                    compiled = false;
                                                    continue
                                                },
                                                "compiled" => {
                                                    compiled = true;
                                                    continue
                                                },
                                                _ => ()
                                            }
                                        }
                                    }
                                }
                                print_exit(1,"",true);
                                compiled = verify_mode(&datapath);
                                print!("Managing package as ");
                                match compiled{
                                    true => print!("compiled"),
                                    false => print!("source")
                                }
                                print_exit(0," ",true);
                            }
                            print!("Saving blueprint...");
                            match &oldv{
                                Some(oldv) => {
                                    match fs::remove_file(format!("{}bps/{}-{}.gz",EOKA_LOCAL,pkgname,oldv)){
                                        Ok(_) => (),
                                        Err(e) => {
                                            print_exit(1,"",true);
                                            println!("Unable to delete previous blueprint -> {}",e);
                                        }
                                    }
                                },
                                None => (),
                            }
                            let cmd = Command::new("/bin/gzip").args([&fname]).output().expect("Unable to execute command");//TODO Formatear estas lineas con [ok] y tal
                            if cmd.status.success() { 
                                match fs::copy(fname+".gz",&format!("{}bps/{}-{}.gz",EOKA_LOCAL,pkgname,pkgversion)){
                                    Ok(_) => print_exit(0,"",true),
                                    Err(e) => {
                                        print_exit(1,"",true);
                                        println!("-> {}", e);
                                        clean_pkg(false,tempdir,&pkg);
                                        return false
                                    }
                                }
                            }else{
                                print_exit(1,"",true);
                                println!("-> {}", String::from_utf8(cmd.stderr).unwrap());
                                clean_pkg(false,tempdir,&pkg);
                                return false
                            }
                        }
                        "conf.sh"|"post.sh" => {
                            print!("Executing {}...",&pfile);
                            let method:&str = if upgrade{
                                "update"
                            }else{
                                "install"
                            };
                            let mut cmd = Command::new("/bin/sh");
                            cmd.args([&fname,method]);
                            if pfile == "conf.sh"{
                                let (mut reader, writer) = os_pipe::pipe().unwrap();
                                let writer_clone = writer.try_clone().unwrap();
                                cmd.stdout(writer);
                                cmd.stderr(writer_clone);
                                let mut handle = cmd.spawn().unwrap();
                                drop(cmd);
                                let mut output = String::new();
                                match reader.read_to_string(&mut output){
                                    Ok(_) => (),
                                    Err(e) => {
                                        print!("Unable to pipe command output due to -> {}",e);
                                        print_exit(1,"",true);
                                        clean_pkg(false,tempdir,&pkg);
                                        return false
                                    }
                                }
                                let res = handle.wait().unwrap();
                                if ! res.success(){
                                    print_exit(1,"",true);
                                    for line in output.split("\n"){
                                        println!("{}",line);
                                    }
                                    print!("conf.sh returned an error, aborting installation...");
                                    clean_pkg(false,tempdir,&pkg);
                                    return false
                                }else{
                                    conf_stdout=Some("Ci");
                                }
                            }else{
                                let mut cmd = cmd.spawn().expect("Unable to execute");
                                if ! cmd.wait().unwrap().success(){
                                    print_exit(1,"",true);
                                    continue;
                                }
                            }
                            print_exit(0,"",true);
                        },
                        "data.tgz" => {
                            if !compiled{
                                if conf_stdout.is_none(){
                                    print!("Package is malformed (source without conf.sh)");
                                    print_exit(1," ",true);
                                    print!("Aborting installation and cleaning enviorment...");
                                    clean_pkg(false,tempdir,&pkg);
                                    return false
                                }
                                print!("Skipping due to package not being precompiled");
                                print_exit(2," ",true);
                                continue
                            }
                            print!("Unpacking source...");
                            output = Command::new("/bin/tar").args(["-xzhf",&fname,"-C","/"]).output().expect("Failed to execute");
                            if ! output.status.success(){
                                print_exit(1," ",false);
                                println!("An error ocurred while extracting data \n{}", String::from_utf8(output.stderr).unwrap());
                                println!("{}",format!("Package {}-{} could not be installed",&pkgname, &pkgversion).red());
                                print!("Removing temportal files and aborting...");
                                clean_pkg(false,tempdir,&pkg);
                                return false
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
        return false
    }
    if upgrade {
        for _ in 0..2{
            let file = OpenOptions::new()
            .append(true)
            .open(EOKA_LOCAL.to_owned()+"lastupdate.log");
            match file{
                Ok(mut file) =>{
                    let timedate:String = {
                        let naive_local = chrono::Local::now().naive_local();
                        format!("{}", naive_local.format("%d/%m/%Y-%H:%M"))
                    };
                    writeln!(file,"{}",format!("[{}] {} {} -> {}",timedate,pkgname,LOCAL.list_pkg(LOCAL.new_con(),&pkgname)[0].lversion,pkgversion)).unwrap();
                    break;
                }
                Err(e) =>{
                    print!("Unable to open the log file");
                    if e.kind().to_string() == "entity not found"{
                        println!("\n Creating new");
                        match File::create(EOKA_LOCAL.to_owned()+"lastupdate.log"){
                            Ok(_) => print_exit(0,"",true),
                            Err(e) => {
                                print_exit(1,"",true);
                                println!("{}",format!(" ⌙--> {}",e).red().bold());

                            }
                        }
                    }else{
                        println!("-> {}",e.kind());
                    }
                }
            }
        }
    }
    LOCAL.update_pkg(LOCAL.new_con(), &pkgname.to_string(), &pkgversion.to_string(), "lversion",&None);
    println!("{}",format!("Package {}-{} has been installed correctly",&pkgname, &pkgversion).green());
    print!("Cleaning enviorment...");
    clean_pkg(true,tempdir,&pkg);
    true
}
async fn reqwest_dependencies(bpdeps:&String,mirrors:&mut Vec<String>) -> Option<Vec<Vec<reqwest::Response>>>{
    let mut result:Vec<Vec<Response>>=Vec::new();
    let bpdeps:Vec<&str>=bpdeps.split(":").collect();
    let mut deps:Vec<Response> = Vec::new();
    for dep in bpdeps[0].split(","){
        if dep == ""{
            continue;
        }
        let dep_v:String;
        let split:Vec<&str> = dep.split("=").collect();
        if split.len() == 1{
            let db_v = LOCAL.list_pkg(LOCAL.new_con(),dep);
            dep_v=db_v[0].rversion.clone();
        }else{
            dep_v=split[1].to_string();
        }
        match check_fs(&format!("{}-{}.ek",dep,dep_v),mirrors).await{
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
    result.push(opts);
    Some(result)
}
async fn package_install(package:String,mirrors:&mut Vec<String>,mut upgrade:bool) -> bool{
    let pkgname = package.to_string();
    let mut oldv:Option<String> = None;
    let mut pkg:Vec<&str> = {
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
            return false;
        }
    };
    if &dbversion.name == ""{
        print_exit(1,"",false);
        println!("  Unable to find package {} try again after eoka sync", pkg[0]);
        return false;
    }else{
        if pkg.len() == 1{
            pkg.push(&dbversion.rversion);
        }
    }
    if &dbversion.lversion == pkg[1]{
        println!("The package {} is already in the latest version {}",&dbversion.name,&dbversion.lversion);
        println!("Do you want to reinstall it? y/N");
        if ! user_input("n"){
            return false;
        } 
    }else if &dbversion.lversion != "0"{
        println!("The package {} is installed with version {} and you are trying to install {}",&dbversion.name,&dbversion.lversion,pkg[1]);
        println!("Do you want to upgrade/downgrade it? y/N");
        if ! user_input("n"){
            return false;
        }
        upgrade=true;
        oldv=Some(dbversion.lversion.clone());
    }else{
        if upgrade{
            println!("Package {} is not installed, do you want to install it? Y/n",package);
            if ! user_input("y"){
                return false;
            }
            upgrade=false;
        }
    }
    if upgrade {
        println!("Upgrading package {}", pkg[0]);
        oldv=Some(dbversion.lversion.clone());
    }else{
        println!("Installing package {}", pkg[0]);
    }
    let bpdeps = dbversion.deps.clone();
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
    let tgzname:&str = &format!("{}-{}.ek",pkg[0],pkg[1]);
    let resp:Response = match check_fs(tgzname,mirrors).await{
        Some(resp) => {
            if resp.status().as_str() == "404"{
                print_exit(1,"",true);
                println!("File not found on this mirror");
                return false
            }
            print_exit(0,"",true);
            resp
        },
        None => {
            println!("{}",format!("Package {} with version {} not found",pkg[0],pkg[1]).red());
            return false
        }
    };
    let fops:Vec<Response>;
    let mut fdeps:Vec<Response> = Vec::new();
    match deps{
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
                return false
            }
        },
    }
    println!("Eoka will need to download {} to install {} continue? Y/n",human_bytes(resp.content_length().expect("Failed to retrieve fs") as f64),pkg[0]);
    if ! user_input("y"){
        return false;
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
            unpack(&dep_n,&dep_v,get_file(dep,&format!("{}-{}.ek",dep_n,dep_v)).await, mirrors, upgrade,&oldv).await;
        }
    }
    unpack(pkg[0],pkg[1], get_file(resp,tgzname).await, mirrors,upgrade,&oldv).await
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
    println!("Are you sure you want to continue? Y/n");
    if ! user_input("y"){
        return
    }
    if package.len() == 1{
        package.push(&dbpkg.lversion);
    }
    let bpath:&str=&format!("{}bps/{}-{}",EOKA_LOCAL,&package[0],&package[1]);
    let cmd = Command::new("/bin/gzip").args([&(bpath.to_owned()+".gz"),"-d","--force"]).output().expect("Unable to execute");
    if ! cmd.status.success(){
        print_exit(1,"",false);
        println!("Unable to unpack {}.gz",bpath);
    }
    let lines:Vec<String> = FS.read_file(bpath.to_string());

    if lines.len() == 0{
        print_exit(1,"",false);
        if package[1] != &dbpkg.lversion{
            println!("Package {} is installed with version {}\nTry again with eoka remove {}={} or eoka remove {}",&package[0],&dbpkg.lversion,&package[0],&dbpkg.lversion,&package[0])
        }else{
            println!("Package {} is installed but no blueprint exist or is corrupted ({})", &package[0],bpath);
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
                                    continue;
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
                                        let path:String = {
                                            if path.chars().nth(0).unwrap() == '/'{
                                                path.to_string()
                                            }else{
                                                "/".to_owned()+path
                                            }
                                        };
                                        match fs::remove_dir_all(&path){
                                            Err(e) => {
                                                match e.to_string().split("(").nth(1).unwrap(){
                                                    "os error 20)" => {
                                                        match fs::remove_file(&path){
                                                            Err(e) => {
                                                                println!("This path could not be removed, aborting due to {}",e);
                                                                return Some(Vec::new())
                                                            },
                                                            Ok(_) => (),
                                                        }
                                                    },
                                                    "os error 2)" =>{
                                                        continue
                                                    }
                                                    e => {
                                                        println!("This path could not be removed, aborting due to {}",e);
                                                        return Some(Vec::new())
                                                    }
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
                        uk => println!("Unknown line {}",uk),
                    }
                },
                
            }
        }
        return None
    }
    match iter(&package[0],lines,fetch){
        Some(deps) => {
            if deps.len() == 0{
                print_exit(1,"",false);
                return
            }
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
            println!("\n{}",format!("Package {} has been uninstalled",&package[0]).green());
        }
    }
    print!("Flushing database ");
    LOCAL.update_pkg(LOCAL.new_con(), &package[0], &"0".to_string(), "lversion", &dbpkg.deps);
    print_exit(0,"",true);
    print!("Cleaning system ");
    match fs::remove_file(format!("{}bps/{}-{}",EOKA_LOCAL,package[0],package[1])){
        Ok(_) => print_exit(0,"",true),
        Err(e) => {
            print_exit(1,"",true);
            println!("{}",e);
        },
    }
    return
}
fn print_deps(pdeps:&Option<String>){
    println!("\n - Dependencies:");
    match pdeps{
        Some(deps) =>{
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
        None => println!("    None"),
    }
}
#[tokio::main]
async fn main(){
    unsafe{
        if geteuid() != 0 {
            println!("{}","Program is not running as root".red());
            println!("This may cause unexpected behaviours, do you still want to proceed? y/N");
            if ! user_input("n"){
                return
            }
        }
    }
    let mut args:Vec<String> = env::args().collect();
    for x in 1..args.len(){
        match args[x].as_str(){
            "-S" | "update" | "sync" =>{
                let mut mirrors:Vec<String> = fetch_mirrors().await;
                println!("Syncing the database");
                let res:Option<Response> = check_fs("eoka.db",&mut mirrors).await;
                get_file(res.expect(""),&REMOTE.name).await;

                if ! hash_check("eoka.db",&mut mirrors).await {
                    print!("File integrity check failed, aborting update...");
                    print_exit(1,"",true);
                    return;
                }else{
                    print!("File integrity passed ");
                    print_exit(0,"",true);
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
                        if cmp_v(&lpackage.rversion, &pack.rversion){
                            LOCAL.update_pkg(LOCAL.new_con(), &pack.name, &pack.rversion, "rversion", &pack.deps);
                            if &pack.lversion != "0"{
                                u+=1;
                            }
                        }
                    }
                    pb.set_message(pack.name);
                    pb.inc(1);
                }
                pb.finish_with_message("Finished updating the dabase");
                match fs::remove_file(&REMOTE.path){
                    Ok(_) => (),
                    Err(e) => {
                        println!("Unable to remove {} due to {}",&REMOTE.path,e);
                    }
                }
                println!("Update endeded with {} new packages, and {} new updates available.",&n,&u);
                return
            },
            "-U"|"upgrade" => {
                let mut mirrors:Vec<String> = fetch_mirrors().await;
                if args.len() == x+1{
                    let mut up = 0;
                    for package in LOCAL.list_pkg(LOCAL.new_con(),"%"){
                        if package.lversion == "0" || package.lversion == package.rversion || package.rversion == "" {
                            //revisar
                            continue;
                        }
                        if package_install(package.name,&mut mirrors,true).await{
                            up+=1;
                        }
                    }
                    println!("Packages upgraded: {}",up);
                }else{
                    let packages:Vec<String>=args.drain(x+1..).collect();
                    for pak in packages{
                        package_install(pak,&mut mirrors,true).await;
                    }
                    return
                }
            },
            "-I"|"install"|"get" => {
                let packages:Vec<String>=args.drain(x+1..).collect();
                let mut mirrors:Vec<String> = fetch_mirrors().await;
                for package in packages{
                    package_install(package,&mut mirrors,false).await;
                }
                return
            },
            "-Q"|"query"=>{
                let packages:Vec<String>=args.drain(x+1..).collect();
                for package in packages{
                    let package:&Package = &LOCAL.list_pkg(LOCAL.new_con(),&package)[0];
                    println!("");
                    println!("{}",format!("Package {} -----------", &package.name).cyan());
                    if &package.lversion == &package.rversion{
                        print!(" - Local version: {}",&package.lversion);
                    }else{
                        if &package.lversion != "0"{
                            print!(" - Local version: {} => Last version: {}", &package.lversion, &package.rversion);
                        }else{
                            print!(" - Available version: {}", &package.rversion);
                        }
                    }
                    print_deps(&package.deps);
                }
                return
            }
            "-L"|"list" => {
                if args.len() == x+1{
                    args.push("all".to_string());
                }
                let mut tot = 0;
                println!("Listing {} packages:",args[x+1]);
                match args[x+1].as_str(){
                    "upgradable" =>{
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
                    },
                    "upgraded"|"updated" =>{
                        for line in FS.read_file(EOKA_LOCAL.to_owned()+"lastupdate.log"){
                            println!("{}",line);
                        }
                    },
                    "all" =>{
                        for package in LOCAL.list_pkg(LOCAL.new_con(),"%"){
                            tot+=1;
                            println!("");
                            let installed:&str = if package.lversion != "0"{
                                " (installed)"
                            }else{
                                ""
                            };
                            println!("{}",format!("Package {}{} -----------", &package.name,installed).cyan());
                            print!(" - Local version: {} => Available version: {}", &package.lversion, &package.rversion);
                            print_deps(&package.deps);
                        }
                    }
                    e => {
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
                return
            }
            _ => println!("Argument {} unrecognised.",args[x])
        }
    }
    
}
