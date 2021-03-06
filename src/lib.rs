/*
    dyn-wall-rs 1.0
    Rehan Rana <rehanalirana@tuta.io>
    Helps user set a dynamic wallpaper and lockscreen. For more info and help, go to https://github.com/RAR27/dyn-wall-rs
    Copyright (C) 2020  Rehan Rana

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */
use crate::time_track::Time;
use chrono::{Local, Timelike};
use clokwerk::{Scheduler, TimeUnits};
use std::{error::Error, process, process::Command, sync::Arc, thread::sleep, time::Duration};
use walkdir::{IntoIter, WalkDir};

use crate::errors::{ConfigFileErrors, Errors};

pub mod errors;
pub mod time_track;

///function that simply changes wallpaper based on the current time in relation to
/// the vector of times passed as an argument
///
/// # Arguments
///
/// * `dir` - path to target directory
/// * `dir_count` - number of files within the directory
/// * `program` - Option containing a string for the user defined program. None if user doesn't pass program
/// * `times` - vector of time objects representing the times for each wallpaper in order
pub fn wallpaper_current_time(
    dir: &str,
    dir_count: usize,
    program: Arc<Option<String>>,
    times: &[Time],
) -> Result<(), Box<dyn Error>> {
    let mut dir_iter = sorted_dir_iter(dir);

    dir_iter
        .next()
        .unwrap()
        .map_err(|_| Errors::DirNonExistantError(dir.to_string()))?;

    let mut feh_handle = Command::new("feh");
    let feh_handle = feh_handle.arg("--bg-scale");
    let mut prog_handle: Command = Command::new("");
    let mut times_iter = times.iter();
    let curr_time = Time::new(Local::now().hour() * 60 + Local::now().minute());
    let loop_time = times_iter.next();
    let full_time = Time::new(24 * 60);
    let midnight = Time::default();
    let mut next_time = times_iter.next().unwrap_or(&full_time);
    let mut filepath_set: String = String::new();
    let mut last_image = String::new();

    let mut loop_time = error_checking(times, loop_time, dir_count)?;

    //this loop is to find where the current time lays, and adjust the wallpaper based on that
    for file in dir_iter {
        //needed for the case where midnight is passed over in the middle of the stated times
        if loop_time > *next_time {
            loop_time = midnight;
        }
        if loop_time == full_time && *next_time == full_time {
            return Err(Errors::ConfigFileError(ConfigFileErrors::FileTimeMismatch).into());
        }

        let filepath_temp = file.map_err(|_| Errors::FilePathError)?;
        let filepath_temp = filepath_temp.path();

        let last_image_temp = filepath_temp.to_str().unwrap();
        last_image = last_image_temp.to_owned();

        if curr_time >= loop_time && curr_time < *next_time {
            filepath_set.push_str(match filepath_temp.to_str() {
                Some(filepath) => Ok(filepath),
                None => Err(Errors::FilePathError),
            }?);
            feh_handle.arg(&filepath_set);

            //this is to send the file as an argument to the user specified program, if one was specified
            prog_handle_loader(&filepath_set, Arc::clone(&program), &mut prog_handle);
        }
        loop_time = *next_time;
        next_time = times_iter.next().unwrap_or(&full_time);
    }

    //this is for the edge case where the current time is after the last time specified for the day, but before the first one specified for the day
    //in that case, the previous loop would push nothing to filepath_set, and so nothing would be sent to feh
    //what we want in this situation is for the file that is associated with the last time of the day to be sent as an argument to feh,
    //and to the user specified program
    if filepath_set.is_empty() {
        feh_handle.arg(&last_image);

        prog_handle_loader(&last_image, Arc::clone(&program), &mut prog_handle);
        filepath_set = last_image;
    }

    feh_handle
        .spawn()
        .map_err(|_| Errors::ProgramRunError(String::from("feh")))?;
    println!("The image {} has been set as your wallpaper", filepath_set);

    if let Some(prog) = program.as_deref() {
        prog_handle
            .spawn()
            .map_err(|_| Errors::ProgramRunError(String::from(prog)))?;
        println!(
            "The image {} has been sent as an argument to the specified program",
            filepath_set
        );
    }
    Ok(())
}

fn prog_handle_loader(filepath_set: &str, program: Arc<Option<String>>, prog_handle: &mut Command) {
    if let Some(prog_str) = program.as_deref() {
        let mut prog_split = prog_str.split_whitespace();
        *prog_handle = Command::new(prog_split.next().unwrap());
        for word in prog_split {
            prog_handle.arg(word);
        }
        prog_handle.arg(filepath_set);
    }
}

pub fn wallpaper_listener(
    dir: String,
    dir_count: usize,
    program: Arc<Option<String>>,
    times_arg: Option<Vec<Time>>,
) -> Result<(), Box<dyn Error>> {
    let (_, step_time, mut loop_time, mut times) = listener_setup(dir.as_str());
    let step_time = step_time?;
    let mut scheduler = Scheduler::new();
    let mut sched_addto = scheduler.every(1.day()).at("0:00");

    match times_arg {
        None => {
            for _ in 1..=dir_count {
                times.push(loop_time);
                loop_time += step_time;
            }
        }
        Some(t) => times = t,
    }

    wallpaper_current_time(&dir, dir_count, Arc::clone(&program), &times)?;

    for time in &times {
        let time_fmt = format!("{:02}:{:02}", time.hours, time.mins);
        sched_addto = sched_addto.and_every(1.day()).at(time_fmt.as_str());
    }

    let sched_closure = move || {
        let result = wallpaper_current_time(&dir, dir_count, Arc::clone(&program), &times);

        match result {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", e);
                process::exit(1);
            }
        }
    };

    sched_addto.run(sched_closure);

    loop {
        scheduler.run_pending();
        sleep(Duration::from_millis(1000));
    }
}

pub fn listener_setup(dir: &str) -> (usize, Result<Time, Errors>, Time, Vec<Time>) {
    let dir_count = WalkDir::new(dir).into_iter().count() - 1;
    let step_time = if dir_count == 0 {
        Err(Errors::NoFilesFoundError(dir.to_string()))
    } else {
        Ok(Time::new(((24.0 / dir_count as f32) * 60.0) as u32))
    };
    let loop_time = Time::default();
    let times: Vec<Time> = Vec::new();

    (dir_count, step_time, loop_time, times)
}

pub fn print_schedule(dir: &str, dir_count: usize) -> Result<(), Box<dyn Error>> {
    let mut dir_iter = sorted_dir_iter(dir);
    let step_time = Time::new(((24.0 / dir_count as f32) * 60.0) as u32);
    let mut loop_time = Time::default();
    let mut i = 0;

    if 1440 % dir_count != 0 || dir_count == 0 {
        return Err(Errors::CountCompatError(dir_count).into());
    }

    dir_iter
        .next()
        .unwrap()
        .map_err(|_| Errors::DirNonExistantError(dir.to_string()))?;

    while i < 24 * 60 {
        println!(
            "Image: {:?} Time: {}",
            dir_iter.next().unwrap()?.file_name(),
            loop_time.twelve_hour()
        );
        i += step_time.total_mins;

        loop_time += step_time;
    }
    Ok(())
}

fn sorted_dir_iter(dir: &str) -> IntoIter {
    WalkDir::new(dir)
        .sort_by(|a, b| {
            alphanumeric_sort::compare_str(
                a.path().to_str().expect("Sorting directory files failed"),
                b.path().to_str().expect("Sorting directory files failed"),
            )
        })
        .into_iter()
}

fn error_checking(
    times: &[Time],
    loop_time: Option<&Time>,
    dir_count: usize,
) -> Result<Time, Box<dyn Error>> {
    let times_iter_err = times.iter();
    let full_time = Time::new(24 * 60);
    let start_range = times.iter().next().ok_or(Errors::ConfigFileError(ConfigFileErrors::Empty))?;
    let mut start_range_other = times.iter().next().unwrap();
    let mut curr_range = start_range.to_owned();
    let mut curr_range_other = start_range.to_owned();
    let mut other_inited = false;
    for time in times_iter_err {
        if *time > *start_range && *time > curr_range {
            curr_range = *time;
        } else if *time > *start_range && *time < curr_range {
            return Err(Errors::ConfigFileError(ConfigFileErrors::OutOfOrder).into());
        } else if *time < *start_range {
            if !other_inited {
                curr_range = full_time;
                start_range_other = time;
                curr_range_other = *time;
                other_inited = true
            } else if *time > *start_range_other && *time > curr_range_other {
                curr_range_other = *time;
            } else {
                return Err(Errors::ConfigFileError(ConfigFileErrors::OutOfOrder).into());
            }
        }
    }

    let loop_time = match loop_time {
        None => Err(Errors::ConfigFileError(ConfigFileErrors::Empty)),
        Some(time) => Ok(time),
    }?;

    if 1440 % dir_count != 0 || dir_count == 0 {
        return Err(Errors::CountCompatError(dir_count).into());
    };
    Ok(*loop_time)
}
