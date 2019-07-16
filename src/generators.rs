use std::io;
use std::io::Write;
use std::rc::Rc;

use crate::charsets::Charset;
use crate::mask::{parse_mask, MaskOp};
use crate::stackbuf::StackBuf;
use crate::wordlists::Wordlist;
use crate::MAX_WORD_SIZE;

pub trait WordGenerator {
    fn gen<'b>(&self, out: Option<Box<dyn Write + 'b>>) -> Result<(), std::io::Error>;
    fn combinations(&self) -> u64;
}

/// Generator optimized for charsets only
pub struct CharsetGenerator<'a> {
    pub mask: &'a str,
    pub minlen: usize,
    pub maxlen: usize,
    charsets: Vec<Charset>,
    min_word: Vec<u8>,
}

/// Wordlist Generator for both charsets and wordlists
pub struct WordlistGenerator<'a> {
    pub mask: &'a str,
    items: Vec<WordlistItem>,
}

#[allow(clippy::large_enum_variant)]
enum WordlistItem {
    Charset(Charset),
    Wordlist(Rc<Wordlist>),
}

enum Position<'a> {
    CharsetPos {
        charset: &'a Charset,
        chr: u8,
    },
    WordlistPos {
        wordlist: &'a Rc<Wordlist>,
        idx: usize,
    },
}

/// returns the correct word generator based on the args provided
pub fn get_word_generator<'a>(
    mask: &'a str,
    minlen: Option<usize>,
    maxlen: Option<usize>,
    custom_charsets: &[&'a str],
    wordlists_fnames: &[&'a str],
) -> Result<Box<dyn WordGenerator + 'a>, &'static str> {
    if wordlists_fnames.is_empty() {
        Ok(Box::new(CharsetGenerator::new(
            mask,
            minlen,
            maxlen,
            custom_charsets,
        )?))
    } else if minlen.is_some() || maxlen.is_some() {
        Err("cannot set minlen or maxlen with wordlists")
    } else {
        Ok(Box::new(WordlistGenerator::new(
            mask,
            wordlists_fnames,
            custom_charsets,
        )?))
    }
}

impl<'a> CharsetGenerator<'a> {
    pub fn new(
        mask: &'a str,
        minlen: Option<usize>,
        maxlen: Option<usize>,
        custom_charsets: &[&'a str],
    ) -> Result<CharsetGenerator<'a>, &'static str> {
        let mask_ops = parse_mask(mask)?;

        // TODO: return error from custom_charset not in index & invalid symbol
        let charsets: Vec<_> = mask_ops
            .into_iter()
            .map(|op| match op {
                MaskOp::Char(ch) => Charset::from_chars(vec![ch as u8].as_ref()),
                MaskOp::BuiltinCharset(ch) => Charset::from_symbol(ch),
                MaskOp::CustomCharset(idx) => Charset::from_chars(custom_charsets[idx].as_bytes()),
                MaskOp::Wordlist(_) => unreachable!("cant handle wordlists"),
            })
            .collect();

        // min/max pwd length is by default the longest word
        let minlen = minlen.unwrap_or_else(|| charsets.len());
        let maxlen = maxlen.unwrap_or_else(|| charsets.len());

        // validate minlen
        if !(0 < minlen && minlen <= maxlen && minlen <= charsets.len()) {
            return Err("minlen is invalid");
        }
        if maxlen > charsets.len() {
            return Err("maxlen is invalid");
        }

        // prepare min word - the longest first word
        let min_word: Vec<u8> = charsets.iter().map(|c| c.min_char).collect();

        Ok(CharsetGenerator {
            mask,
            charsets,
            minlen,
            maxlen,
            min_word,
        })
    }

    #[allow(clippy::borrowed_box)]
    fn gen_by_length<'b>(
        &self,
        pwdlen: usize,
        out: &mut Box<dyn Write + 'b>,
    ) -> Result<(), std::io::Error> {
        let mut buf = StackBuf::new();
        let batch_size = buf.len() / (pwdlen + 1);

        let word = &mut [b'\n'; MAX_WORD_SIZE][..=pwdlen];
        word[..pwdlen].copy_from_slice(&self.min_word[..pwdlen]);

        'outer_loop: loop {
            'batch_for: for _ in 0..batch_size {
                buf.write(word);
                for pos in (0..pwdlen).rev() {
                    let chr = word[pos];
                    let next_chr = self.charsets[pos][chr as usize];
                    word[pos] = next_chr;

                    if chr < next_chr {
                        continue 'batch_for;
                    }
                }
                break 'outer_loop;
            }

            out.write_all(&buf.getdata())?;
            buf.clear();
        }
        out.write_all(buf.getdata())?;
        Ok(())
    }
}

impl<'a> WordGenerator for CharsetGenerator<'a> {
    /// generates all words into the output buffer `out`
    fn gen<'b>(&self, out: Option<Box<dyn Write + 'b>>) -> Result<(), std::io::Error> {
        let mut out = out.unwrap_or_else(|| Box::new(io::stdout()));

        for pwdlen in self.minlen..=self.maxlen {
            self.gen_by_length(pwdlen, &mut out)?;
        }
        Ok(())
    }

    /// calculates number of words to be generated by this WordGenerator
    fn combinations(&self) -> u64 {
        let mut combs = 0;
        for i in self.minlen..=self.maxlen {
            combs += self
                .charsets
                .iter()
                .take(i)
                .fold(1, |acc, x| acc * x.chars.len() as u64);
        }
        combs
    }
}

impl<'a> WordlistGenerator<'a> {
    pub fn new(
        mask: &'a str,
        wordlists_fnames: &[&'a str],
        custom_charsets: &[&'a str],
    ) -> Result<WordlistGenerator<'a>, &'static str> {
        let mask_ops = parse_mask(mask)?;

        // TODO: split to functions
        let mut wordlists_data = vec![];
        for fname in wordlists_fnames.iter() {
            wordlists_data.push(Rc::new(Wordlist::from_file(fname).expect("invalid fname")));
        }

        // TODO: return error from custom_charset not in index & invalid symbol
        let items: Vec<WordlistItem> = mask_ops
            .into_iter()
            .map(|op| match op {
                MaskOp::Char(ch) => {
                    WordlistItem::Charset(Charset::from_chars(vec![ch as u8].as_ref()))
                }
                MaskOp::BuiltinCharset(ch) => WordlistItem::Charset(Charset::from_symbol(ch)),
                MaskOp::CustomCharset(idx) => {
                    WordlistItem::Charset(Charset::from_chars(custom_charsets[idx].as_bytes()))
                }
                MaskOp::Wordlist(idx) => WordlistItem::Wordlist(Rc::clone(&wordlists_data[idx])),
            })
            .collect();

        Ok(WordlistGenerator { mask, items })
    }

    #[allow(clippy::borrowed_box)]
    fn gen_words<'b>(&self, out: &mut Box<dyn Write + 'b>) -> Result<(), std::io::Error> {
        let mut buf = StackBuf::new();

        let mut word_buf = [b'\n'; MAX_WORD_SIZE];
        let word = &mut word_buf[..];
        let mut positions: Vec<_> = self
            .items
            .iter()
            .map(|item| match item {
                WordlistItem::Charset(charset) => Position::CharsetPos {
                    charset,
                    chr: charset.min_char,
                },
                WordlistItem::Wordlist(wordlist) => Position::WordlistPos { wordlist, idx: 0 },
            })
            .collect();

        let mut min_word = vec![];
        for pos in positions.iter() {
            match pos {
                Position::CharsetPos { chr, .. } => min_word.push(*chr),
                Position::WordlistPos { wordlist, .. } => min_word.extend_from_slice(&wordlist[0]),
            }
        }
        min_word.push(b'\n');
        let min_word = min_word;
        let mut word_len = min_word.len();

        word[..word_len].copy_from_slice(&min_word);

        'outer_loop: loop {
            if buf.pos() + word_len >= buf.len() {
                out.write_all(&buf.getdata())?;
                buf.clear();
            }
            buf.write(&word[..word_len]);

            let mut pos = word_len - 2;

            for itempos in positions.iter_mut().rev() {
                match itempos {
                    Position::CharsetPos { charset, chr } => {
                        let prev_chr = *chr;
                        *chr = charset[prev_chr as usize];
                        word[pos] = *chr;

                        if prev_chr < *chr {
                            continue 'outer_loop;
                        }

                        // TODO: this is because test has overflow check
                        if pos == 0 {
                            break 'outer_loop;
                        }
                        pos -= 1;
                    }
                    Position::WordlistPos { wordlist, idx } => {
                        let prev_len = wordlist[*idx].len();
                        *idx += 1;
                        if *idx == wordlist.len() {
                            *idx = 0;
                        }

                        let wlen = wordlist[*idx].len();

                        // TODO: try simplify this routine
                        if prev_len == wlen {
                            word[pos + 1 - wlen..=pos].copy_from_slice(&wordlist[*idx]);
                            if pos >= wlen {
                                pos -= wlen;
                            } else {
                                pos = 0;
                            }
                        } else {
                            let offset = wlen as isize - prev_len as isize;

                            // move the suffix by offset (can be negative)
                            let after_word = pos + 1;
                            let tmp = word[after_word..word_len].to_vec();
                            word[(after_word as isize + offset) as usize
                                ..(word_len as isize + offset) as usize]
                                .copy_from_slice(&tmp);

                            // update current position & wordlien by offset
                            pos = (pos as isize + offset) as usize;
                            word_len = (word_len as isize + offset) as usize;

                            // copy the next word (similar to prev_len == wlen block)
                            word[pos + 1 - wlen..=pos].copy_from_slice(&wordlist[*idx]);
                            if pos >= wlen {
                                pos -= wlen;
                            } else {
                                pos = 0;
                            }
                        }

                        // if idx == 0 we finished the wordlist
                        if *idx > 0 {
                            continue 'outer_loop;
                        }
                    }
                }
            }

            // done
            break;
        }
        out.write_all(buf.getdata())?;
        Ok(())
    }
}

impl<'a> WordGenerator for WordlistGenerator<'a> {
    /// generates all words into the output buffer `out`
    fn gen<'b>(&self, out: Option<Box<dyn Write + 'b>>) -> Result<(), std::io::Error> {
        let mut out = out.unwrap_or_else(|| Box::new(io::stdout()));

        self.gen_words(&mut out)?;
        Ok(())
    }

    fn combinations(&self) -> u64 {
        self.items
            .iter()
            .map(|item| match item {
                WordlistItem::Wordlist(wl) => wl.len() as u64,
                WordlistItem::Charset(c) => c.chars.len() as u64,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::{CharsetGenerator, WordGenerator};
    use std::fs;
    use std::io::Cursor;
    use std::path;

    #[test]
    fn test_gen_words_single_digit() {
        let mask = "?d";
        let word_gen = CharsetGenerator::new(mask, None, None, &vec![]).unwrap();

        assert_eq!(word_gen.mask, mask);
        assert_eq!(word_gen.minlen, 1);
        assert_eq!(word_gen.maxlen, 1);
        assert_eq!(word_gen.charsets.len(), 1);
        assert_eq!(word_gen.min_word, "0".as_bytes());

        let res = assert_gen(word_gen, "single-digits.txt");

        // paranoid test of assert_gen
        assert_eq!(res, "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    }

    #[test]
    fn test_gen_upper_lower_1_4() {
        let mask = "?u?l?u?l";
        let word_gen = CharsetGenerator::new(mask, Some(1), None, &vec![]).unwrap();

        assert_eq!(word_gen.mask, mask);
        assert_eq!(word_gen.minlen, 1);
        assert_eq!(word_gen.maxlen, 4);
        assert_eq!(word_gen.charsets.len(), 4);
        assert_eq!(word_gen.min_word, "AaAa".as_bytes());

        assert_gen(word_gen, "upper-lower-1-4.txt");
    }

    #[test]
    fn test_gen_pwd_upper_lower_year_1_4() {
        let mask = "pwd?u?l201?1";
        let word_gen = CharsetGenerator::new(mask, Some(1), None, &vec!["56789"]).unwrap();

        assert_eq!(word_gen.mask, mask);
        assert_eq!(word_gen.minlen, 1);
        assert_eq!(word_gen.maxlen, 9);
        assert_eq!(word_gen.charsets.len(), 9);
        assert_eq!(word_gen.min_word, "pwdAa2015".as_bytes());

        assert_gen(word_gen, "upper-lower-year-1-4.txt");
    }

    fn assert_gen(w: CharsetGenerator, fname: &str) -> String {
        let mut buf: Vec<u8> = Vec::new();
        let mut cur = Cursor::new(&mut buf);
        w.gen(Some(Box::new(&mut cur))).unwrap();

        let result = String::from_utf8(buf).unwrap();
        let expected = fs::read_to_string(wordlist_fname(fname)).unwrap();

        assert_eq!(result, expected);
        result
    }

    fn wordlist_fname(fname: &str) -> path::PathBuf {
        let mut d = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.extend(vec!["test-resources", fname]);
        d
    }

    #[test]
    fn test_gen_stats() {
        let custom_charsets = vec!["abcd", "01"];
        let combinations = vec![
            ("?d?s?u?l?a?b", 5368197120, None, None),
            ("?d?d?d?d?d?d?d?d", 111111110, Some(1), Some(8)),
            ("?d?d?d?d?d?d?d?d", 10000, Some(4), Some(4)),
            ("?d?d?d?d?d?d?d?d", 100000000, None, Some(8)),
            ("?1?2", 8, None, None),
            ("?d?1?2", 80, None, None),
            ("?d?s?u?l?a?b?1?2", 42945576960, None, None),
            ("?d?1?2?d", 930, Some(1), None),
        ];

        for (mask, result, minlen, maxlen) in combinations {
            let word_gen = CharsetGenerator::new(mask, minlen, maxlen, &custom_charsets).unwrap();
            assert_eq!(word_gen.combinations(), result);
        }
    }
}
