extern crate avg32;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate anyhow;
extern crate lexpr;
extern crate serde_lexpr;
#[macro_use]
extern crate log;
extern crate clap;
extern crate env_logger;

#[cfg(test)]
extern crate pretty_assertions;

mod disasm;

use anyhow::Result;
use avg32::archive::{self, Archive};
use avg32::font;
use avg32::write::Writeable;
use clap::{crate_authors, crate_version, App, Arg, ArgMatches, SubCommand};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

fn get_app<'a, 'b>() -> App<'a, 'b> {
    App::new("adieu")
        .version(crate_version!())
        .author(crate_authors!())
        .about("AVG32 bytecode disassembler/reassembler")
        .subcommand(
            SubCommand::with_name("unpack")
                .about("Unpack a SEEN.TXT file")
                .arg(
                    Arg::with_name("output-dir")
                        .short("o")
                        .long("output-dir")
                        .help("output directory")
                        .takes_value(true)
                        .value_name("DIR"),
                )
                .arg(
                    Arg::with_name("raw")
                        .short("r")
                        .long("raw")
                        .help("don't automatically dissassemble files"),
                )
                .arg(
                    Arg::with_name("FILE")
                        .required(true)
                        .help("SEEN.TXT file")
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("repack")
                .about("Packs a directory to a SEEN.TXT file")
                .arg(
                    Arg::with_name("output-dir")
                        .short("o")
                        .long("output-dir")
                        .help("output directory")
                        .takes_value(true)
                        .value_name("DIR"),
                )
                .arg(
                    Arg::with_name("raw")
                        .short("r")
                        .long("raw")
                        .help("don't automatically assemble files"),
                )
                .arg(
                    Arg::with_name("DIR")
                        .required(true)
                        .help("Directory containing bytecode files")
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("disasm")
                .about("Disassemble an AVG32 scene")
                .arg(
                    Arg::with_name("output-dir")
                        .short("o")
                        .long("output-dir")
                        .help("output directory")
                        .takes_value(true)
                        .value_name("DIR"),
                )
                .arg(
                    Arg::with_name("FILE")
                        .required(true)
                        .help("SEEN<XXX>.TXT file")
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("asm")
                .about("Assemble a .adieu source into an AVG32 scene")
                .arg(
                    Arg::with_name("output-dir")
                        .short("o")
                        .long("output-dir")
                        .help("output directory")
                        .takes_value(true)
                        .value_name("DIR"),
                )
                .arg(
                    Arg::with_name("FILE")
                        .required(true)
                        .help("SEEN<XXX>.adieu file")
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("font").about("Reads FN.DAT").arg(
                Arg::with_name("FILE")
                    .required(true)
                    .help("FN.DAT file")
                    .index(1),
            ),
        )
}

fn cmd_unpack(sub_matches: &ArgMatches) -> Result<()> {
    let input_file = Path::new(sub_matches.value_of("FILE").unwrap());
    let output_dir = match sub_matches.value_of("output-dir") {
        Some(dir) => Path::new(dir),
        None => input_file.parent().unwrap(),
    };
    let raw = sub_matches.is_present("raw");

    fs::create_dir_all(output_dir)?;
    let arc = archive::load(&input_file)?;

    for (i, entry) in arc.entries.iter().enumerate() {
        let data = &arc.data[i];
        let decomp = data.decompress()?;
        if raw {
            let output_file = output_dir.join(&entry.filename);
            let mut file = File::create(&output_file)?;
            decomp.write(&mut file)?;
        } else {
            let scene = avg32::load_bytes(&decomp)?;
            let output_file =
                output_dir.join(PathBuf::from(&entry.filename).with_extension("adieu"));
            let mut file = File::create(&output_file)?;
            let sexp = disasm::disassemble(&scene)?;
            file.write_all(&sexp.as_bytes())?;
        }
    }

    println!("Wrote {} files to {:?}.", arc.entries.len(), output_dir);
    Ok(())
}

fn cmd_repack(sub_matches: &ArgMatches) -> Result<()> {
    let input_dir = Path::new(sub_matches.value_of("DIR").unwrap());
    let output_dir = match sub_matches.value_of("output-dir") {
        Some(dir) => Path::new(dir),
        None => input_dir.parent().unwrap(),
    };
    let raw = sub_matches.is_present("raw");

    let mut arc = Archive::new();

    for entry in fs::read_dir(input_dir)? {
        let entry = entry?;
        let path = entry.path();

        let metadata = fs::metadata(&path)?;
        if metadata.is_file() {
            let scene = if raw {
                avg32::load(&path)?
            } else {
                let sexp = fs::read_to_string(&path)?;
                disasm::assemble(&sexp)?
            };

            let mut bytes = Vec::new();
            scene.write(&mut bytes)?;
            let comp = archive::compress(&bytes)?;

            let filename = String::from(
                path.with_extension("TXT")
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            );
            arc.add_entry(filename, comp)?;
        }
    }

    let output_file = output_dir.join("SEEN.TXT");
    let mut file = File::create(&output_file)?;
    arc.finalize();
    arc.write(&mut file)?;

    println!("Packed {} files to {:?}.", arc.entries.len(), output_file);
    Ok(())
}

fn cmd_disasm(sub_matches: &ArgMatches) -> Result<()> {
    let input_file = Path::new(sub_matches.value_of("FILE").unwrap());
    let output_dir = match sub_matches.value_of("output-dir") {
        Some(dir) => Path::new(dir),
        None => input_file.parent().unwrap(),
    };

    let scene = avg32::load(&input_file.to_str().unwrap())?;
    let sexp = disasm::disassemble(&scene)?;

    let output_file = output_dir.join(input_file.with_extension("adieu").file_name().unwrap());
    let mut file = File::create(&output_file)?;
    file.write_all(&sexp.as_bytes())?;

    println!("Dissassembled bytecode to {:?}.", output_file);
    Ok(())
}

fn cmd_asm(sub_matches: &ArgMatches) -> Result<()> {
    let input_file = Path::new(sub_matches.value_of("FILE").unwrap());
    let output_dir = match sub_matches.value_of("output-dir") {
        Some(dir) => Path::new(dir),
        None => input_file.parent().unwrap(),
    };

    let sexp = fs::read_to_string(&input_file)?;
    let scene = disasm::assemble(&sexp)?;

    let output_file = output_dir.join(input_file.with_extension("TXT").file_name().unwrap());
    let mut file = File::create(&output_file)?;
    scene.write(&mut file)?;

    println!("Assembled bytecode to {:?}.", output_file);
    Ok(())
}

fn cmd_font(sub_matches: &ArgMatches) -> Result<()> {
    let input_file = Path::new(sub_matches.value_of("FILE").unwrap());

    let font = font::load(&input_file)?;

    let print = |c| match c {
        0 => print!("\""),
        1 => print!("@"),
        2 => print!("@"),
        3 => print!("%"),
        4 => print!("#"),
        5 => print!("*"),
        6 => print!("+"),
        7 => print!("="),
        8 => print!("="),
        9 => print!("-"),
        10 => print!("-"),
        11 => print!(":"),
        12 => print!(":"),
        13 => print!("."),
        14 => print!(" "),
        15 => print!(" "),
        _ => print!(" "),
    };

    for char in font.chars.iter() {
        for y in 0..24 {
            for x in 0..12 {
                let c = char[y * 12 + x];
                let a = (c & 0xF0) >> 4;
                let b = c & 0x0F;

                print(a);
                print(b);
            }
            println!()
        }
        println!("======================")
    }

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();

    let matches = get_app().get_matches();

    match matches.subcommand() {
        ("unpack", Some(sub_matches)) => cmd_unpack(&sub_matches)?,
        ("repack", Some(sub_matches)) => cmd_repack(&sub_matches)?,
        ("disasm", Some(sub_matches)) => cmd_disasm(&sub_matches)?,
        ("asm", Some(sub_matches)) => cmd_asm(&sub_matches)?,
        ("font", Some(sub_matches)) => cmd_font(&sub_matches)?,
        _ => get_app().print_long_help()?,
    }

    Ok(())
}
