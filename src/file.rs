use std::io::{Read, Write, self};
use std::thread;
use std::time::Duration;
use std::fs::{self, File};
use std::process::Command;
use std::env;
use std::path::PathBuf;
use std::string::String;
use std::option::Option;
use protocol::*;

const REMOTEPATH_ALPINE_RELEASE: &'static str = ".alpine-release";
const REMOTEPATH_TEST_BURN: &'static str = "mpc_testburn";
const FILE_PREFIX: &'static str = "result-";
const TMP_RESULT_LOC: &'static str = "mpc-result";

/// Clears the entire terminal screen, moves cursor to top left.
pub fn reset() {
    print!("{}[2J", 27 as char);
    print!("{}[1;1H", 27 as char);
    println!("[MPC] Do not exit this process or shut the system off.");
    println!("");
}

pub fn prompt(s: &str) -> String {
    loop {
        let mut input = String::new();
        reset();
        println!("{}", s);
        println!("\x07");

        if io::stdin().read_line(&mut input).is_ok() {
            println!("Please wait...");
            return (&input[0..input.len()-1]).into();
        }
    }
}

pub fn getFilePath(suffix: &str) -> Option<String>{
    match env::home_dir() {
        Some(path) => {
            let mut p = format!("{}{}{}{}", path.to_str().unwrap(), TMP_RESULT_LOC, FILE_PREFIX, suffix);
            return Some(p);
        },
        None => {
            return None;
        }
    }
}

pub struct TemporaryFile {
    path: String,
    f: Option<File>
}

impl TemporaryFile {
    pub fn reset(&mut self) {
        self.f = None;
        self.f = Some(File::open(&self.path).unwrap());
    }
}

impl Read for TemporaryFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.f.as_mut().unwrap().read(buf)
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        // Close the file descriptor...
        self.f = None;

        // Delete the file.
        loop {
            if fs::remove_file(&self.path).is_ok() {
                return;
            }
            println!("Failed to remove file! Trying again...");
            thread::sleep(Duration::from_secs(1));
        }
    }
}

pub fn write_to_file(file_path: &str) -> bool {
    println!("Saving...");
    File::create(file_path);
    return true;
}

pub enum FileStatus {
    File(TemporaryFile),
    Empty,
    Error
}

pub fn read_from_file(file_path: &str) -> FileStatus {
    match File::open(file_path) {
        Ok(f) => {
            if f.metadata().is_ok() && f.metadata().unwrap().len() == 0{
                return FileStatus::Empty
            } else {
                return FileStatus::File(TemporaryFile {
                    path: file_path.into(),
                    f: Some(f)
                })
            }
        },
        Err(_) => {
            println!("Error opening file at path {}", file_path);
            return FileStatus::Error
        }
    }
}

pub fn hash_of_file<R: Read>(f: &mut R) -> Digest256 {
    Digest256::from_reader(f)
}

pub fn exchange_file<
    T,
    R1,
    R2,
    F1: Fn(&mut File) -> Result<(), R1>,
    F2: Fn(&mut TemporaryFile, Option<Digest256>) -> Result<T, R2>
>(
    our_file: &str,
    their_file: &str,
    our_cb: F1,
    their_cb: F2
) -> T
{   
    let newfile = getFilePath(our_file).unwrap();
    let newfile_localpath = newfile.as_str();
    {
        let mut newfile = File::create(newfile_localpath).unwrap();
        our_cb(&mut newfile).ok().unwrap();
    }
    if ::ASK_USER_TO_RECORD_HASHES {
        let mut newfile = File::open(newfile_localpath).unwrap();
        let h = hash_of_file(&mut newfile);

        write_down_file_please(&h, our_file);
    }

    let mut already_saved = false;

    loop {
        if already_saved {
            prompt(&format!("Insert file '{}' from the other machine. If the burn of file '{}' failed,\n\
                             insert another blank file to burn it again. Press [ENTER] when ready.",
                            their_file, our_file));
        } else {
            prompt(&format!("File '{}' will be saved. Press [ENTER] to start.",
                            our_file));
        }

        match read_from_file(getFilePath(their_file).unwrap().as_str()) {
            FileStatus::File(mut f) => {
                let h;
                if ::ASK_USER_TO_RECORD_HASHES {
                    h = Some(hash_of_file(&mut f));
                    write_down_file_please(&h.clone().unwrap(), their_file);
                    f.reset();
                } else {
                    h = None;
                }

                match their_cb(&mut f, h) {
                    Ok(data) => {
                        //let _ = fs::remove_file(newfile_localpath);

                        return data;
                    },
                    Err(_) => {
                        prompt(&format!("The file '{}' you inserted may be corrupted. Burn it again \
                                         on the other machine. Then insert the new file '{}' and \
                                         press [ENTER].", their_file, their_file));
                    }
                }
            },
            FileStatus::Empty => {
                write_to_file(newfile_localpath);
                prompt(&format!("file {} has been created. Label the file and transfer it to the\n\
                                 other machine. Press [ENTER] when the drive is clear.", our_file));
            },
            FileStatus::Error => {
                prompt(&format!("Error!!!"));
            }
        }
    }
}

pub fn write_file<
    R,
    F: Fn(&mut File) -> Result<(), R>
>(
    our_file: &str,
    our_cb: F
)
{
    let newfile = getFilePath(our_file).unwrap();
    let newfile_localpath = newfile.as_str();
    {
        let mut newfile = File::create(newfile_localpath).unwrap();
        our_cb(&mut newfile).ok().unwrap();
    }
    if ::ASK_USER_TO_RECORD_HASHES {
        let mut newfile = File::open(newfile_localpath).unwrap();
        let h = hash_of_file(&mut newfile);
        write_down_file_please(&h, our_file);
    }

    
    match read_from_file(newfile_localpath) {
        FileStatus::File(mut f) => {
            write_to_file(newfile_localpath);
            prompt(&format!("file {} has been burned. Label the file and transfer it to the\n\
                             other machine. Press [ENTER] when the drive is clear.", our_file));
        },
        _ => {
            //TODO: handle error
        }
    }
}

pub fn read_file<T, R, F: Fn(&mut TemporaryFile, Option<Digest256>) -> Result<T, R>>(name: &str, message: &str, cb: F) -> T {
    prompt(message);

    loop {
        match read_from_file(name) {
            FileStatus::File(mut f) => {
                let h;
                if ::ASK_USER_TO_RECORD_HASHES {
                    h = Some(hash_of_file(&mut f));
                    write_down_file_please(&h.clone().unwrap(), name);
                    f.reset();
                } else {
                    h = None;
                }

                match cb(&mut f, h) {
                    Ok(data) => {
                        return data;
                    },
                    Err(_) => {
                        prompt(&format!("The file you inserted may be corrupted. Create it again \
                                on the other machine.\n\n{}", message));
                    }
                }
            },
            FileStatus::Empty => {
                prompt(&format!("You placed a blank File, but we're expecting \
                                 file '{}'.\n\n{}", name, message));
            },
            FileStatus::Error => {
                prompt(&format!("ERROR"));
            }
        }
    }
}

pub fn write_down_file_please(h: &Digest256, name: &str) {
    // TODO: in the future, this will be written to the blockchain and we will wait until it is there
    loop {
        if "recorded" == prompt(&format!("Please write down and publish the string: {}\n\
                                          It is the hash of file '{}'.\n\n\
                                          Type 'recorded' and press [ENTER] to confirm you've written it down.",
                                          h.to_string(),
                                          name)) {
            break;
        }
    }
}
