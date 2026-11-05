//! Integrity-checked filesystem checkpoints and portable experiment packages.
use crate::{create_experiment, delete_experiment, inspect_experiment, manifest::Manifest, platform::PlatformInfo, workspace::Workspace, CathedralError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Component, Path, PathBuf}};
use uuid::Uuid;

const VERSION:u32=1;
const MAGIC:&[u8]=b"CATHEDRAL-PACKAGE-V1\n";
const MAX_META:u64=16*1024*1024;
const MAX_FILE:u64=4*1024*1024*1024;
const MAX_TOTAL:u64=16*1024*1024*1024;

#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum EntryKind { File, Directory }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct Entry { pub path:String, pub kind:EntryKind, pub size:u64, pub sha256:Option<String> }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct Host { pub os:String, pub arch:String }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct Lockfile {
 pub format_version:u32,
 pub manifest_sha256:String,
 pub host:Host,
 pub runtime_backend:String,
 pub dependencies:std::collections::BTreeMap<String,String>,
}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct Metadata {
 pub version:u32, pub snapshot_id:String, pub experiment_id:String, pub name:String,
 pub created_at:i64, pub manifest:Manifest, pub host:Host, pub entries:Vec<Entry>
}
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct Summary { pub id:String, pub name:String, pub files:usize, pub directories:usize, pub bytes:u64, pub created_at:i64 }
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct Difference { pub path:String, pub change:String }
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct PackageReport { pub name:String, pub id:String, pub source:Host, pub target:Host, pub dependencies:usize, pub compatible:bool }

fn err(s:impl Into<String>)->CathedralError { CathedralError::Manifest(s.into()) }
fn snaproot(root:&Path,id:&str)->PathBuf { root.join("snapshots").join(id) }
fn relative(s:&str)->Result<PathBuf>{
 if s.is_empty()||s.contains('\\')||s.contains(':')||s.as_bytes().contains(&0){return Err(err(format!("unsafe path '{s}'")))}
 let p=Path::new(s);
 if p.is_absolute()||p.components().any(|c|!matches!(c,Component::Normal(_))){return Err(err(format!("unsafe path '{s}'")))}
 Ok(p.to_path_buf())
}
fn checked_existing_path(root:&Path,value:&str)->Result<PathBuf>{
 let mut path=root.to_path_buf();
 for component in relative(value)?.components(){
  path.push(component);
  let metadata=fs::symlink_metadata(&path)?;
  if metadata.file_type().is_symlink(){return Err(err(format!("symlinked snapshot path component: {}",path.display())))}
 }
 Ok(path)
}
fn hash(p:&Path)->Result<(u64,String)>{
 let mut f=File::open(p)?;let mut h=Sha256::new();let mut n=0u64;let mut b=[0u8;65536];
 loop{let k=f.read(&mut b)?;if k==0{break}n+=k as u64;h.update(&b[..k]);}
 Ok((n,format!("{:x}",h.finalize())))
}
fn copy_tree(src:&Path,dst:&Path,rel:&Path,out:&mut Vec<Entry>)->Result<()>{
 let mut children=fs::read_dir(src)?.collect::<std::io::Result<Vec<_>>>()?;children.sort_by_key(|e|e.file_name());
 for e in children{
  let sp=e.path();let rp=rel.join(e.file_name());let text=rp.to_str().ok_or_else(||err("non UTF-8 workspace path"))?.replace('\\',"/");relative(&text)?;
  let m=fs::symlink_metadata(&sp)?;let dp=dst.join(&rp);
  if m.file_type().is_symlink(){return Err(err(format!("symbolic link is not snapshot-safe: {text}")))}
  if m.is_dir(){fs::create_dir(&dp)?;out.push(Entry{path:text,kind:EntryKind::Directory,size:0,sha256:None});copy_tree(&sp,dst,&rp,out)?}
  else if m.is_file(){
   if m.len()>MAX_FILE{return Err(err(format!("file exceeds 4 GiB limit: {text}")))}
   fs::copy(&sp,&dp)?;let (a,b)=hash(&sp)?;let (c,d)=hash(&dp)?;
   if a!=c||b!=d{return Err(err(format!("file changed during snapshot: {text}")))}
   out.push(Entry{path:text,kind:EntryKind::File,size:c,sha256:Some(d)});
  }else{return Err(err(format!("special file is not snapshot-safe: {text}")))}
 }Ok(())
}
fn validate(m:&Metadata)->Result<()>{
 if m.version!=VERSION{return Err(err(format!("unsupported snapshot version {}",m.version)))}
 m.manifest.validate()?;let mut seen=std::collections::HashSet::new();let mut total=0u64;
 for e in &m.entries{
  relative(&e.path)?;if !seen.insert(&e.path){return Err(err(format!("duplicate path {}",e.path)))}
  match e.kind{
   EntryKind::Directory if e.size==0&&e.sha256.is_none()=>{},
   EntryKind::File if e.size<=MAX_FILE&&e.sha256.as_ref().is_some_and(|h|h.len()==64&&h.bytes().all(|b|b.is_ascii_hexdigit()))=>total=total.saturating_add(e.size),
   _=>return Err(err(format!("invalid entry {}",e.path)))
  }
 }if total>MAX_TOTAL{return Err(err("snapshot exceeds 16 GiB limit"))}Ok(())
}
fn lockfile_for(manifest:&Manifest,host:&Host)->Result<Lockfile>{
 let canonical=toml::to_string_pretty(manifest).map_err(|e|err(e.to_string()))?;
 Ok(Lockfile{format_version:1,manifest_sha256:format!("{:x}",Sha256::digest(canonical.as_bytes())),host:host.clone(),runtime_backend:if host.os=="linux"{"bubblewrap-linux".into()}else{"unsupported".into()},dependencies:manifest.dependencies.packages.clone()})
}
fn validate_lockfile(path:&Path,m:&Metadata)->Result<()>{
 let lock:Lockfile=serde_json::from_slice(&fs::read(path)?).map_err(|e|err(format!("invalid cathedral.lock: {e}")))?;
 if lock!=lockfile_for(&m.manifest,&m.host)?{return Err(err("cathedral.lock does not match manifest, dependencies, or host requirements"))}Ok(())
}
fn summary(m:&Metadata)->Summary{Summary{id:m.snapshot_id.clone(),name:m.name.clone(),files:m.entries.iter().filter(|e|e.kind==EntryKind::File).count(),directories:m.entries.iter().filter(|e|e.kind==EntryKind::Directory).count(),bytes:m.entries.iter().map(|e|e.size).sum(),created_at:m.created_at}}
fn metadata(path:&Path)->Result<Metadata>{
 let f=fs::symlink_metadata(path)?;if f.file_type().is_symlink()||!f.is_file()||f.len()>MAX_META{return Err(err("unsafe or oversized snapshot metadata"))}
 let m:Metadata=serde_json::from_slice(&fs::read(path)?).map_err(|e|err(format!("invalid snapshot metadata: {e}")))?;validate(&m)?;Ok(m)
}
pub fn create(root:&Path,name:&str)->Result<Summary>{
 let exp=inspect_experiment(root,name)?;let ws=Workspace::new(root)?.validate(Path::new(&exp.workspace_path))?;
 let manifest=Manifest::load(&ws.join("cathedral.toml"))?;let id=Uuid::new_v4().to_string();let dir=snaproot(root,&exp.id).join(&id);
 fs::create_dir_all(dir.join("files"))?;
 let result=(||->Result<Metadata>{
   let mut entries=Vec::new();copy_tree(&ws,&dir.join("files"),Path::new(""),&mut entries)?;
   let p=PlatformInfo::detect();let host=Host{os:p.operating_system.into(),arch:p.architecture.into()};
   let lock=lockfile_for(&manifest,&host)?;let lock_path=dir.join("files").join("cathedral.lock");
   fs::write(&lock_path,serde_json::to_vec_pretty(&lock).map_err(|e|err(e.to_string()))?)?;
   entries.retain(|entry|entry.path!="cathedral.lock");let(lock_size,lock_hash)=hash(&lock_path)?;
   entries.push(Entry{path:"cathedral.lock".into(),kind:EntryKind::File,size:lock_size,sha256:Some(lock_hash)});
   entries.sort_by(|a,b|a.path.cmp(&b.path));
   let m=Metadata{version:VERSION,snapshot_id:id.clone(),experiment_id:exp.id.clone(),name:exp.name.clone(),created_at:crate::registry::unix_timestamp(),manifest,host,entries};validate(&m)?;
  let tmp=dir.join("metadata.tmp");let mut f=OpenOptions::new().write(true).create_new(true).open(&tmp)?;f.write_all(&serde_json::to_vec_pretty(&m).map_err(|e|err(e.to_string()))?)?;f.sync_all()?;drop(f);fs::rename(tmp,dir.join("snapshot.json"))?;Ok(m)
 })();
 match result{Ok(m)=>Ok(summary(&m)),Err(e)=>{let _=fs::remove_dir_all(dir);Err(e)}}
}
fn load(root:&Path,name:&str,id:&str)->Result<(PathBuf,Metadata)>{
 if id.is_empty()||!id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-'){return Err(err("invalid snapshot ID"))}
 let exp=inspect_experiment(root,name)?;let dir=snaproot(root,&exp.id).join(id);
 let d=fs::symlink_metadata(&dir)?;if d.file_type().is_symlink()||!d.is_dir(){return Err(err("snapshot directory is unsafe"))}
 let m=metadata(&dir.join("snapshot.json"))?;if m.snapshot_id!=id||m.experiment_id!=exp.id{return Err(err("snapshot identity mismatch"))}Ok((dir,m))
}
pub fn list(root:&Path,name:&str)->Result<Vec<Summary>>{
 let exp=inspect_experiment(root,name)?;let dir=snaproot(root,&exp.id);if !dir.exists(){return Ok(vec![])}
 let mut out=Vec::new();for e in fs::read_dir(dir)?{let e=e?;let m=fs::symlink_metadata(e.path())?;if m.file_type().is_symlink()||!m.is_dir(){return Err(err("unsafe snapshot entry"))}let x=metadata(&e.path().join("snapshot.json"))?;if x.experiment_id!=exp.id||x.snapshot_id!=e.file_name().to_string_lossy(){return Err(err("snapshot identity mismatch"))}out.push(summary(&x))}
 out.sort_by_key(|s|s.created_at);Ok(out)
}
pub fn verify(root:&Path,name:&str,id:&str)->Result<Summary>{
 let (dir,m)=load(root,name,id)?;let files=dir.join("files");let root_meta=fs::symlink_metadata(&files)?;
 if root_meta.file_type().is_symlink()||!root_meta.is_dir(){return Err(err("snapshot files directory is unsafe"))}
 for e in &m.entries{let p=checked_existing_path(&files,&e.path)?;let a=fs::symlink_metadata(&p)?;
  if a.file_type().is_symlink(){return Err(err(format!("symlink in snapshot: {}",e.path)))}
  match e.kind{EntryKind::Directory if a.is_dir()=>{},EntryKind::File if a.is_file()=>{let(n,h)=hash(&p)?;if n!=e.size||Some(h)!=e.sha256{return Err(err(format!("integrity failure: {}",e.path)))}}_=>return Err(err(format!("missing or wrong-type entry: {}",e.path)))}
 }validate_lockfile(&files.join("cathedral.lock"),&m)?;Ok(summary(&m))
}
pub fn compare(root:&Path,name:&str,a:&str,b:&str)->Result<Vec<Difference>>{
 verify(root,name,a)?;verify(root,name,b)?;let(_,x)=load(root,name,a)?;let(_,y)=load(root,name,b)?;
 let left:std::collections::BTreeMap<_,_>=x.entries.iter().map(|e|(e.path.as_str(),e)).collect();
 let right:std::collections::BTreeMap<_,_>=y.entries.iter().map(|e|(e.path.as_str(),e)).collect();let mut out=Vec::new();
 for(p,e)in &right{match left.get(p){None=>out.push(Difference{path:(*p).into(),change:"added".into()}),Some(old)if old.kind!=e.kind||old.size!=e.size||old.sha256!=e.sha256=>out.push(Difference{path:(*p).into(),change:"changed".into()}),_=>{}}}
 for p in left.keys(){if !right.contains_key(p){out.push(Difference{path:(*p).into(),change:"removed".into()})}}out.sort_by(|a,b|a.path.cmp(&b.path));Ok(out)
}
pub fn restore(root:&Path,name:&str,id:&str)->Result<()>{
 verify(root,name,id)?;let(dir,m)=load(root,name,id)?;let exp=inspect_experiment(root,name)?;
 let target=Workspace::new(root)?.validate(Path::new(&exp.workspace_path))?;let base=root.join("experiments");
 let stage=base.join(format!(".restore-{}",Uuid::new_v4()));let backup=base.join(format!(".backup-{}",Uuid::new_v4()));fs::create_dir(&stage)?;
 let copied=(||->Result<()>{for e in &m.entries{let r=relative(&e.path)?;let s=checked_existing_path(&dir.join("files"),&e.path)?;let d=stage.join(&r);match e.kind{EntryKind::Directory=>fs::create_dir_all(d)?,EntryKind::File=>{if let Some(p)=d.parent(){fs::create_dir_all(p)?}fs::copy(s,&d)?;let(n,h)=hash(&d)?;if n!=e.size||Some(h)!=e.sha256{return Err(err(format!("file changed during restore: {}",e.path)))}}}}Ok(())})();
 if let Err(e)=copied{let _=fs::remove_dir_all(&stage);return Err(e)}
 fs::rename(&target,&backup)?;if let Err(e)=fs::rename(&stage,&target){let rb=fs::rename(&backup,&target);let _=fs::remove_dir_all(&stage);return match rb{Ok(())=>Err(e.into()),Err(r)=>Err(err(format!("restore failed: {e}; rollback failed: {r}")))}}fs::remove_dir_all(backup)?;Ok(())
}
fn package_write(path:&Path,m:&Metadata,files:&Path)->Result<()>{
 validate(m)?;let json=serde_json::to_vec(m).map_err(|e|err(e.to_string()))?;let mut o=OpenOptions::new().write(true).create_new(true).open(path)?;
 o.write_all(MAGIC)?;o.write_all(&(json.len()as u64).to_le_bytes())?;o.write_all(&json)?;
 for e in &m.entries{if e.kind==EntryKind::File{std::io::copy(&mut File::open(files.join(relative(&e.path)?))?,&mut o)?;}}o.sync_all()?;Ok(())
}
pub fn export(root:&Path,name:&str,out:&Path)->Result<PackageReport>{
 let s=create(root,name)?;verify(root,name,&s.id)?;let exp=inspect_experiment(root,name)?;let(dir,m)=load(root,name,&s.id)?;
 let parent=out.parent().unwrap_or(Path::new("."));fs::create_dir_all(parent)?;let tmp=parent.join(format!(".{}.{}.tmp",out.file_name().unwrap_or_default().to_string_lossy(),Uuid::new_v4()));
 if let Err(e)=package_write(&tmp,&m,&dir.join("files")){let _=fs::remove_file(tmp);return Err(e)}fs::rename(tmp,out)?;
 let p=PlatformInfo::detect();let target=Host{os:p.operating_system.into(),arch:p.architecture.into()};Ok(PackageReport{name:exp.name,id:exp.id,source:m.host.clone(),compatible:m.host==target,target,dependencies:m.manifest.dependencies.packages.len()})
}
fn package_read(path:&Path,stage:&Path)->Result<Metadata>{
 let mut i=File::open(path)?;let mut magic=vec![0;MAGIC.len()];i.read_exact(&mut magic)?;if magic!=MAGIC{return Err(err("unsupported Cathedral package"))}
 let mut len=[0;8];i.read_exact(&mut len)?;let n=u64::from_le_bytes(len);if n==0||n>MAX_META{return Err(err("invalid package metadata length"))}
 let mut json=vec![0;n as usize];i.read_exact(&mut json)?;let m:Metadata=serde_json::from_slice(&json).map_err(|e|err(format!("invalid package metadata: {e}")))?;validate(&m)?;
 let mut total=0u64;for e in &m.entries{let r=relative(&e.path)?;let d=stage.join(r);match e.kind{EntryKind::Directory=>fs::create_dir_all(d)?,EntryKind::File=>{total=total.saturating_add(e.size);if total>MAX_TOTAL{return Err(err("package exceeds 16 GiB limit"))}if let Some(p)=d.parent(){fs::create_dir_all(p)?}let mut o=OpenOptions::new().write(true).create_new(true).open(&d)?;let mut left=e.size;let mut h=Sha256::new();let mut b=[0;65536];while left>0{let k=left.min(b.len()as u64)as usize;i.read_exact(&mut b[..k])?;o.write_all(&b[..k])?;h.update(&b[..k]);left-=k as u64;}o.sync_all()?;if Some(format!("{:x}",h.finalize()))!=e.sha256{return Err(err(format!("package integrity failure: {}",e.path)))}}}}
 let mut extra=[0];if i.read(&mut extra)?!=0{return Err(err("unexpected trailing package data"))}
 if Manifest::load(&stage.join("cathedral.toml"))?!=m.manifest{return Err(err("embedded manifest does not match package metadata"))}validate_lockfile(&stage.join("cathedral.lock"),&m)?;Ok(m)
}
pub fn import(root:&Path,package:&Path,strict:bool)->Result<PackageReport>{
 fs::create_dir_all(root.join("imports"))?;let stage=root.join("imports").join(Uuid::new_v4().to_string());fs::create_dir(&stage)?;
 let m=match package_read(package,&stage){Ok(m)=>m,Err(e)=>{let _=fs::remove_dir_all(&stage);return Err(e)}};
 let p=PlatformInfo::detect();let target=Host{os:p.operating_system.into(),arch:p.architecture.into()};let compatible=m.host==target;
 if strict&&!compatible{let _=fs::remove_dir_all(&stage);return Err(err(format!("host mismatch: package {} / {}, current {} / {}",m.host.os,m.host.arch,target.os,target.arch)))}
 let exp=match create_experiment(root,&m.name,m.manifest.clone()){Ok(x)=>x,Err(e)=>{let _=fs::remove_dir_all(&stage);return Err(e)}};
 let result=(||->Result<()>{for e in &m.entries{let r=relative(&e.path)?;let s=stage.join(&r);let d=Path::new(&exp.workspace_path).join(&r);match e.kind{EntryKind::Directory=>fs::create_dir_all(d)?,EntryKind::File=>{if let Some(p)=d.parent(){fs::create_dir_all(p)?}if d.exists(){fs::remove_file(&d)?}fs::copy(s,d)?;}}}Ok(())})();
 let _=fs::remove_dir_all(&stage);if let Err(e)=result{let _=delete_experiment(root,&m.name);return Err(e)}
 Ok(PackageReport{name:exp.name,id:exp.id,source:m.host,compatible,target,dependencies:m.manifest.dependencies.packages.len()})
}

#[cfg(test)]
mod tests{
 use super::*;use tempfile::tempdir;
 #[test]fn snapshot_verify_compare_restore(){let t=tempdir().unwrap();let e=create_experiment(t.path(),"snap-lab",Manifest::minimal("snap-lab")).unwrap();let w=Path::new(&e.workspace_path);fs::write(w.join("a.txt"),"one").unwrap();let a=create(t.path(),"snap-lab").unwrap();fs::write(w.join("a.txt"),"two").unwrap();fs::write(w.join("b.txt"),"new").unwrap();let b=create(t.path(),"snap-lab").unwrap();assert!(verify(t.path(),"snap-lab",&a.id).is_ok());let d=compare(t.path(),"snap-lab",&a.id,&b.id).unwrap();assert!(d.iter().any(|x|x.path=="a.txt"&&x.change=="changed"));assert!(d.iter().any(|x|x.path=="b.txt"&&x.change=="added"));restore(t.path(),"snap-lab",&a.id).unwrap();assert_eq!(fs::read_to_string(w.join("a.txt")).unwrap(),"one");assert!(!w.join("b.txt").exists());}
 #[test]fn package_roundtrip(){let a=tempdir().unwrap();let b=tempdir().unwrap();let e=create_experiment(a.path(),"portable-lab",Manifest::minimal("portable-lab")).unwrap();fs::write(Path::new(&e.workspace_path).join("data.txt"),"portable").unwrap();let p=a.path().join("test.cath");export(a.path(),"portable-lab",&p).unwrap();let r=import(b.path(),&p,true).unwrap();assert!(r.compatible);let x=inspect_experiment(b.path(),"portable-lab").unwrap();assert_eq!(fs::read_to_string(Path::new(&x.workspace_path).join("data.txt")).unwrap(),"portable");assert!(Path::new(&x.workspace_path).join("cathedral.lock").is_file());}
 #[cfg(unix)]#[test]fn verify_rejects_symlinked_snapshot_parent(){use std::os::unix::fs::symlink;let t=tempdir().unwrap();let e=create_experiment(t.path(),"link-lab",Manifest::minimal("link-lab")).unwrap();let w=Path::new(&e.workspace_path);fs::create_dir(w.join("nested")).unwrap();fs::write(w.join("nested/data"),"safe").unwrap();let s=create(t.path(),"link-lab").unwrap();let files=snaproot(t.path(),&e.id).join(&s.id).join("files");fs::remove_dir_all(files.join("nested")).unwrap();let outside=t.path().join("outside");fs::create_dir(&outside).unwrap();fs::write(outside.join("data"),"external").unwrap();symlink(&outside,files.join("nested")).unwrap();assert!(verify(t.path(),"link-lab",&s.id).is_err());}
 #[test]fn detects_corruption(){let t=tempdir().unwrap();let e=create_experiment(t.path(),"hash-lab",Manifest::minimal("hash-lab")).unwrap();fs::write(Path::new(&e.workspace_path).join("x"),"good").unwrap();let s=create(t.path(),"hash-lab").unwrap();fs::write(snaproot(t.path(),&e.id).join(&s.id).join("files/x"),"bad").unwrap();assert!(verify(t.path(),"hash-lab",&s.id).is_err());}
}
