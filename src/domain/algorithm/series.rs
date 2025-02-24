use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesDetails {
    pub series_title: String,
    pub season: String,
    pub episode: String,
    pub episode_title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesDetailsFull {
    pub series_details: SeriesDetails,
    pub func_name: String,
    pub rule: String,
    pub index: i32,
}

impl SeriesDetailsFull {
    pub fn new(
        series_title: String,
        season: String,
        episode: String,
        episode_title: String,
        func_name: String,
        rule: String,
        index: i32,
    ) -> Self {
        SeriesDetailsFull {
            series_details: SeriesDetails {
                series_title,
                season,
                episode,
                episode_title,
            },
            func_name,
            rule,
            index,
        }
    }
}

lazy_static! {
    static ref PATH_REGEX: Vec<Regex> = vec![
        Regex::new(r"^(?P<series_title>[\s\w]+) [Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2}) - (?P<episode_title>[^\\/\n]+)").unwrap(),
        Regex::new(r"[Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2}) - (?P<episode_title>[^\\/\n]+)").unwrap(),
        Regex::new(r".*\d+-(?P<episode>\d{1,2}) (?P<episode_title>[^\\/\n]+)").unwrap(),
        Regex::new(r".*[Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2})[- ]+(?P<episode_title>[^\\/\n]+)").unwrap(),
        Regex::new(r"^(?P<series_title>[^\\/\n]+) (?P<season>\d{1,2})-(?P<episode>\d{1,2}) (?P<episode_title>[^\\/\n]+)$").unwrap(),
        Regex::new(r"(?P<series_title>[^\\/\n]+)/.*[Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2})").unwrap(),
        Regex::new(r"^(?P<series_title>[^\\/\n]+) [Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2})").unwrap(),
        Regex::new(r"^[Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2}) - (?P<series_title>[^\\/\n]+)").unwrap(),
        Regex::new(r"^(?P<episode_title>[\s\w]+) part (?P<episode>\d{1,2})").unwrap(),
        Regex::new(r"^(?P<episode>\d{1,2})[\s\-]+(?P<episode_title>[\s\w]+)").unwrap(),
    ];

    static ref SEASON_STRIP_REGEX: Vec<Regex> = vec![
        Regex::new(r"^(?P<seasonName>[\s\w]+)[Ss]\d{1,2}").unwrap(),
        Regex::new(r"^(?P<seasonName>[\s\w]+)Season\s*\d{1,2}").unwrap(),
    ];

    static ref FILENAME_REGEX: Vec<Regex> = vec![
        Regex::new(r"^(?P<series_title>[^\\/\n]+) [Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2})[\-\s]+(?P<episode_title>[\w\s]+)").unwrap(),
        Regex::new(r"^(?P<series_title>[^\\/\n]+) [Ss](?P<season>\d{1,2})[Ee](?P<episode>\d{1,2})").unwrap(),
        Regex::new(r"^(?P<series_title>[\s\w',]+)\s*[-]+\s*(?P<episode>\d{1,2})$").unwrap(),
        Regex::new(r"^(?P<series_title>[\s\w',]+)\s*[-]+\s*(?P<episode>\d{1,2})\D+").unwrap(),
        Regex::new(r"^(?P<series_title>[\s\w',]+)\s*[-]+\s*(?P<episode_title>[\s\w\-]+)").unwrap(),
        Regex::new(r"^(?P<series_title>[\s\w',]+).*?PART (?P<episode>\d{1,2}).*?(?P<episode_title>[\s\w\-]+)").unwrap(),
        Regex::new(r"^(?P<series_title>[^&]+)&(?P<episode_title>[\s\w\-']+)").unwrap(),
        Regex::new(r#"^(?P<series_title>[\s\w'",]+)\s+[\d]{4}"#).unwrap(),
    ];
}

pub fn parse_file_path(file_name: &str) -> SeriesDetailsFull {
    let file_name = file_name.trim_start_matches('/');
    let parts: Vec<&str> = file_name.split('/').collect();
    
    let series_title = extract_season(&parts[0]);
    if series_title == "New" || series_title == "YouTube" || parts.len() == 1 {
        return parse_only_file_name(file_name);
    }
    if series_title.is_empty() {
        return parse_only_file_name(parts[parts.len() - 1]);
    }

    let mut season = String::new();
    if parts.len() > 2 {
        season = remove_leading_and_trailing_characters(&parts[1]);
    }

    let episode_part = parts[parts.len() - 1];
    let base_name = Path::new(episode_part)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let base_name = base_name.replace('.', " ");

    for (index, regex) in PATH_REGEX.iter().enumerate() {
        if let Some(caps) = regex.captures(&base_name) {
            let mut season = season.clone();
            if let Some(found_season) = caps.name("season") {
                season = remove_leading_and_trailing_characters(found_season.as_str());
            } else if season.is_empty() {
                season = "1".to_string();
            }

            let episode = add_leading_zero(
                &remove_leading_and_trailing_characters(
                    caps.name("episode")
                        .map(|m| m.as_str())
                        .unwrap_or("")
                )
            );

            let mut episode_title = caps.name("episode_title")
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            episode_title = make_title(&episode_title);

            if episode_title == episode {
                return SeriesDetailsFull::new(
                    series_title,
                    season,
                    String::new(),
                    episode_title,
                    "parse_file_path".to_string(),
                    regex.as_str().to_string(),
                    index as i32,
                );
            }

            return SeriesDetailsFull::new(
                series_title,
                season,
                episode,
                episode_title,
                "parse_file_path".to_string(),
                regex.as_str().to_string(),
                index as i32,
            );
        }
    }

    let mut episode = String::new();
    if !season.is_empty() {
        episode = remove_leading_and_trailing_characters(&base_name);
    }

    let base_name = base_name.trim_start_matches(&series_title);
    let episode_title = remove_trailing_characters(&make_title(base_name));
    
    if episode == episode_title {
        episode = String::new();
    }

    SeriesDetailsFull::new(
        series_title,
        make_title(&season),
        add_leading_zero(&episode),
        episode_title,
        "parse_file_path".to_string(),
        String::new(),
        -1,
    )
}

fn extract_season(season: &str) -> String {
    let season = season.replace('.', " ");
    for regex in SEASON_STRIP_REGEX.iter() {
        if let Some(caps) = regex.captures(&season) {
            if let Some(new_season) = caps.name("seasonName") {
                return make_title(new_season.as_str());
            }
        }
    }
    make_title(&season)
}

pub fn parse_only_file_name(file_name: &str) -> SeriesDetailsFull {
    let base_name = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let base_name = base_name.replace('.', " ");
    //let base_name = base_name.replace('"', "'");
    let base_name = remove_after_encoding_abbrv(&base_name);

    for (index, regex) in FILENAME_REGEX.iter().enumerate() {
        if let Some(caps) = regex.captures(&base_name) {
            let series_title = make_title(
                caps.name("series_title")
                    .map(|m| m.as_str())
                    .unwrap_or("")
            );

            let season = caps.name("season")
                .map(|m| m.as_str().trim_start_matches('0'))
                .unwrap_or("");

            let episode = add_leading_zero(
                &remove_leading_and_trailing_characters(
                    caps.name("episode")
                        .map(|m| m.as_str().trim_start_matches('0'))
                        .unwrap_or("")
                )
            );

            let mut episode_title = caps.name("episode_title")
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            episode_title = make_title(&remove_leading_and_trailing_characters(&episode_title));

            if episode_title == episode {
                return SeriesDetailsFull::new(
                    series_title,
                    season.to_string(),
                    String::new(),
                    episode_title,
                    "parse_only_file_name".to_string(),
                    regex.as_str().to_string(),
                    index as i32,
                );
            }

            return SeriesDetailsFull::new(
                series_title,
                season.to_string(),
                episode,
                episode_title,
                "parse_only_file_name".to_string(),
                regex.as_str().to_string(),
                index as i32,
            );
        }
    }

    SeriesDetailsFull::new(
        make_title(&base_name),
        String::new(),
        String::new(),
        String::new(),
        "parse_only_file_name".to_string(),
        String::new(),
        -1,
    )
}

fn remove_leading_and_trailing_characters(s: &str) -> String {
    remove_letter_and_leading_zeros(&remove_trailing_characters(s))
}

fn remove_letter_and_leading_zeros(s: &str) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^[a-zA-Z\s]*0{0,1}(\d+.*)").unwrap();
    }
    RE.replace(s, "$1").to_string()
}

fn remove_trailing_characters(s: &str) -> String {
    let s = s.trim_end();
    let chars: Vec<char> = s.chars().collect();
    
    let mut last_valid_index = chars.len() as i32 - 1;
    while last_valid_index >= 0 {
        let c = chars[last_valid_index as usize];
        if c.is_alphanumeric() || c == '}' || c == ']' || c == ')' || c == '?' {
            break;
        }
        last_valid_index -= 1;
    }

    if last_valid_index < 0 {
        return String::new();
    }

    if last_valid_index == 0 || last_valid_index == chars.len() as i32 - 1 {
        return s.to_string();
    }

    s[..=last_valid_index as usize].to_string()
}

fn remove_after_first_special_char(input: &str) -> String {
    let mut result = input.to_string();
    for (i, c) in input.char_indices() {
        if c.is_whitespace() || c.is_digit(10) || c.is_alphabetic() {
            continue;
        }
        if c == '(' || c == '[' || c == ')' || c == ']' || c == '+' {
            result = input[..i].to_string();
            break;
        }
    }
    result
}

fn remove_leading_special_chars(input: &str) -> String {
    input.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string()
}

fn remove_after_encoding_abbrv(input: &str) -> String {
    let abbrvs = ["x264", "rb58", "1080", "1080p", "h264", "x265", "h265",
                  "720", "720p", "xvid", "dvdrip", "ws", "pdtv"];
    
    let input_words: Vec<&str> = input.split_whitespace().collect();
    let lc_words: Vec<String> = input_words.iter()
        .map(|w| w.to_lowercase())
        .collect();

    let mut earliest_pos = usize::MAX;
    
    for abbr in abbrvs.iter() {
        if let Some(pos) = lc_words.iter().position(|word| word == abbr) {
            if pos == 0 {
                return String::new();
            }
            if pos < earliest_pos {
                earliest_pos = pos;
            }
        }
    }
    
    if earliest_pos != usize::MAX {
        return input_words[..earliest_pos].join(" ");
    }
    
    input.to_string()
}

fn is_all_uppercase(input: &str) -> bool {
    input.chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase())
}

fn is_all_lowercase(input: &str) -> bool {
    input.chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_lowercase())
}

fn capitalize_first_if_lower_case(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }

    let mut chars: Vec<char> = s.chars().collect();
    if let Some(first) = chars.first_mut() {
        if first.is_lowercase() {
            *first = first.to_uppercase().next().unwrap_or(*first);
        }
    }
    chars.into_iter().collect()
}

pub fn make_title(input: &str) -> String {
    let input = input.replace('"', "'");
    let input = remove_leading_special_chars(
        &remove_after_first_special_char(
            &remove_after_encoding_abbrv(&input)
        )
    ).trim().to_string();

    if is_all_lowercase(&input) || is_all_uppercase(&input) {
        input.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        let mut result = first.to_uppercase().to_string();
                        result.extend(chars.flat_map(|c| c.to_lowercase()));
                        result
                    }
                }
            })
            .collect::<Vec<String>>()
            .join(" ")
    } else {
        capitalize_first_if_lower_case(&input)
    }
}

fn add_leading_zero(input: &str) -> String {
    if let Ok(num) = input.parse::<i32>() {
        if num < 10 {
            return format!("0{}", num);
        }
    }
    input.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_file_paths() {
        let tests = vec![
            (
                "Grantchester/Grantchester Christmas Special 2016 720p WEB-DL HEVC x265 BONE.mp4",
                SeriesDetails {
                    series_title: "Grantchester".to_string(),
                    season: "".to_string(),
                    episode_title: "Christmas Special 2016".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Jazz/Chucho Valdés & The Afro Cuban Messengers Chucho's Step.mp4",
                SeriesDetails {
                    series_title: "Jazz".to_string(),
                    season: "".to_string(),
                    episode_title: "Chucho Valdés & The Afro Cuban Messengers Chucho's Step".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "YouTube/Thomas Hardy's \"Tess of the D'Urbervilles\" 1998 FULL Justine Waddell, Oliver Milburn, Jason Flemying - AMT2 0 - Remember?",
                SeriesDetails {
                    series_title: "Thomas Hardy's 'Tess of the D'Urbervilles'".to_string(),
                    season: "".to_string(),
                    episode_title: "".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "The House of Eliott [Stella Gonet,Louise Lombard] [ENG(UK);S1-3complete;DVDrips;RARE]/Season 3/3x08 - (02-20-1993).mp4",
                SeriesDetails {
                    series_title: "The House of Eliott".to_string(),
                    season: "3".to_string(),
                    episode_title: "3x08".to_string(),
                    episode: "3x08 - (02-20-1993)".to_string(),
                },
            ),
            (
                "Poldark/shorts/Forbidden Love.mp4",
                SeriesDetails {
                    series_title: "Poldark".to_string(),
                    season: "Shorts".to_string(),
                    episode_title: "Forbidden Love".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "[ www.Torrenting.com ] - Clockwise.1986.DVDRip.x264-NoRBiT/Clockwise.1986.DVDRip.x264-NoRBiT.mp4",
                SeriesDetails {
                    series_title: "Clockwise".to_string(),
                    season: "".to_string(),
                    episode_title: "".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Star Trek The Next Generation Season 1, 2, 3, 4, 5, 6 & 7 + Extras (1080p BD x265 10bit FS79 Joy)/Star Trek The Next Generation Season 3  (1080p BD x265 10bit FS84 Joy)/Star Trek TNG S02E22 Shades of Gray  (1080p x265 10bit Joy).mp4",
                SeriesDetails {
                    series_title: "Star Trek The Next Generation".to_string(),
                    season: "2".to_string(),
                    episode: "22".to_string(),
                    episode_title: "Shades of Gray".to_string(),
                },
            ),
            (
                "Game.of.Thrones.S01.720p.BluRay.x264.ShAaNiG/Game.of.Thrones.S01E10.720p.BluRay.450MB.ShAaNiG.com.mp4",
                SeriesDetails {
                    series_title: "Game of Thrones".to_string(),
                    season: "1".to_string(),
                    episode: "10".to_string(),
                    episode_title: "".to_string(),
                },
            ),
            (
                "/Chucho Valdés & The Afro Cuban Messengers Chucho's Step.mp4",
                SeriesDetails {
                    series_title: "Chucho Valdés".to_string(),
                    season: "".to_string(),
                    episode_title: "The Afro Cuban Messengers Chucho's Step".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Jazz/Chucho Valdés 'Jazz Batá 2' Tourcoing Jazz Festival.mp4",
                SeriesDetails {
                    series_title: "Jazz".to_string(),
                    season: "".to_string(),
                    episode_title: "Chucho Valdés 'Jazz Batá 2' Tourcoing Jazz Festival".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Bleak House/Bleak.House.S01E13.WS.PDTV.XviD-RiVER.mp4",
                SeriesDetails {
                    series_title: "Bleak House".to_string(),
                    season: "1".to_string(),
                    episode: "13".to_string(),
                    episode_title: "".to_string(),
                },
            ),
            (
                "YouTube/Basic YOGA ASANAS for GOOD HEALTH (PART 2) - for Beginners and all Age Groups ｜ Yoga at Home.mp4",
                SeriesDetails {
                    series_title: "Basic YOGA ASANAS for GOOD HEALTH".to_string(),
                    season: "".to_string(),
                    episode: "02".to_string(),
                    episode_title: "For Beginners and all Age Groups".to_string(),
                },
            ),
            (
                "New/Sherlock.Holms.2009.1080p.BrRip.x264.YIFY.mp4",
                SeriesDetails {
                    series_title: "Sherlock Holms".to_string(),
                    season: "".to_string(),
                    episode_title: "".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "/Tess of the D'Urbervilles - 4-4.mp4",
                SeriesDetails {
                    series_title: "Tess of the D'Urbervilles".to_string(),
                    season: "".to_string(),
                    episode: "04".to_string(),
                    episode_title: "".to_string(),
                },
            ),
            (
                "New/Summer.in.February.2013.1080p.BluRay.x264.YIFY.mp4",
                SeriesDetails {
                    series_title: "Summer in February".to_string(),
                    season: "".to_string(),
                    episode_title: "".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Inspector Morse/Season 1/3 -  Service Of All The Dead.mp4",
                SeriesDetails {
                    series_title: "Inspector Morse".to_string(),
                    season: "1".to_string(),
                    episode: "03".to_string(),
                    episode_title: "Service Of All The Dead".to_string(),
                },
            ),
            (
                "YouTube/Delvon Lamarr Organ Trio - Warm-up Set (Live on KEXP).mp4",
                SeriesDetails {
                    series_title: "Delvon Lamarr Organ Trio".to_string(),
                    season: "".to_string(),
                    episode_title: "Warm-up Set".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Only Fools and Horses/Specials/S00E11 - Miami Twice - The American Dream (Uncut).mp4",
                SeriesDetails {
                    series_title: "Only Fools and Horses".to_string(),
                    season: "0".to_string(),
                    episode: "11".to_string(),
                    episode_title: "Miami Twice - The American Dream".to_string(),
                },
            ),
            (
                "/inspector.morse.s01e02.1080p.web.h264-cbfm[eztv.re].mp4",
                SeriesDetails {
                    series_title: "Inspector Morse".to_string(),
                    season: "1".to_string(),
                    episode: "02".to_string(),
                    episode_title: "".to_string(),
                },
            ),
            (
                "Cranford + Return to Cranford (2007, 09 BBC)/Return to Cranford part 1.mp4",
                SeriesDetails {
                    series_title: "Cranford".to_string(),
                    season: "1".to_string(),
                    episode: "01".to_string(),
                    episode_title: "Return to Cranford".to_string(),
                },
            ),
            (
                "Kung Fu Panda 4 (2024) [1080p] [WEBRip] [5.1] [YTS.MX]",
                SeriesDetails {
                    series_title: "Kung Fu Panda 4".to_string(),
                    season: "".to_string(),
                    episode_title: "".to_string(),
                    episode: "".to_string(),
                },
            ),
            (
                "Inspector Morse S07/Morse S07E01 - Deadly Slumber x264 RB58.mp4",
                SeriesDetails {
                    series_title: "Inspector Morse".to_string(),
                    season: "7".to_string(),
                    episode: "01".to_string(),
                    episode_title: "Deadly Slumber".to_string(),
                },
            ),
            (
                "Line of Duty/Line Of Duty S02E05.mp4",
                SeriesDetails {
                    series_title: "Line of Duty".to_string(),
                    season: "2".to_string(),
                    episode: "05".to_string(),
                    episode_title: "".to_string(),
                },
            ),
            (
                "New/Poirot S01E06 - Triangle at Rhodes (1989)-v2.mp4",
                SeriesDetails {
                    series_title: "Poirot".to_string(),
                    season: "1".to_string(),
                    episode: "06".to_string(),
                    episode_title: "Triangle at Rhodes".to_string(),
                },
            ),
            (
                "The Sweeny/Season 3/The Sweeney 3-10 Pay Off.mp4",
                SeriesDetails {
                    series_title: "The Sweeny".to_string(),
                    season: "3".to_string(),
                    episode: "10".to_string(),
                    episode_title: "Pay Off".to_string(),
                },
            ),
            (
                "Star Trek/Star Trek TOS S03E19 Requiem for Methuselah.mp4",
                SeriesDetails {
                    series_title: "Star Trek".to_string(),
                    season: "3".to_string(),
                    episode: "19".to_string(),
                    episode_title: "Requiem for Methuselah".to_string(),
                },
            ),
            (
                "Star.Trek.Lower.Decks.S01E01.1080p.BluRay.x265-RARBG[eztv.re].mp4",
                SeriesDetails {
                    series_title: "Star Trek Lower Decks".to_string(),
                    season: "1".to_string(),
                    episode: "01".to_string(),
                    episode_title: "".to_string(),
                },
            ),
            (
                "/Star.Trek.Strange.New.Worlds.S02E10.1080p.WEB.h264-ETHEL.mp4",
                SeriesDetails {
                    series_title: "Star Trek Strange New Worlds".to_string(),
                    season: "2".to_string(),
                    episode: "10".to_string(),
                    episode_title: "".to_string(),
                },
            ),
        ];

        let mut rules_used: HashMap<String, HashMap<i32, bool>> = HashMap::new();

        for (input, expected) in tests {
            let got = parse_file_path(input);
            
            if got.series_details != expected {
                if got.series_details.series_title != expected.series_title {
                    panic!("Failed for '{}': Expected series_title '{}', got '{}'",
                        input, expected.series_title, got.series_details.series_title);
                }
                if got.series_details.season != expected.season {
                    panic!("Failed for '{}': Expected season '{}', got '{}'",
                        input, expected.season, got.series_details.season);
                }
                if got.series_details.episode != expected.episode {
                    panic!("Failed for '{}': Expected episode '{}', got '{}'",
                        input, expected.episode, got.series_details.episode);
                }
                if got.series_details.episode_title != expected.episode_title {
                    panic!("Failed for '{}': Expected episode_title '{}', got '{}'",
                        input, expected.episode_title, got.series_details.episode_title);
                }
                panic!("Failed for '{}': Func: {}, Rule: {}, {}",
                    input, got.func_name, got.index, got.rule);
            }

            rules_used.entry(got.func_name)
                .or_insert_with(HashMap::new)
                .insert(got.index, true);
        }

        // Log used rules
        for (method, rules) in rules_used {
            let mut rule_list: Vec<i32> = rules.keys().cloned().collect();
            rule_list.sort();
            println!("{}: {:?}", method, rule_list);
        }
    }

    #[test]
    fn test_make_title() {
        assert_eq!(make_title("test string"), "Test String");
        assert_eq!(make_title("TEST STRING"), "Test String");
        assert_eq!(make_title("test STRING"), "Test STRING");
    }
}
