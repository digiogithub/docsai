//! Parsing an `insert_*` fragment **for the sequence it is going into**.
//!
//! §6.3: «the fragment's kind must match the sequence. Blocks go into a block
//! sequence, rows into a table, slides into a deck, shapes onto a slide, sheets
//! into a workbook. […] There is no coercion.»
//!
//! That sentence is implemented literally, and in the direction it is written:
//! the *sequence* is known first and the fragment is parsed for it. The
//! alternative — parse, then see what came out, then check — reads the same and
//! behaves worse, because DocMark's own dialect selection is contextual (a
//! `## Title {.slide}` is a slide, the same line without the class is a
//! heading), so «what came out» depends on a guess this module would have to
//! make and the target already answers.

use std::path::Path;

use docsai_model::addressing::SeqKind;
use docsai_model::assets::AssetStore;
use docsai_model::image::ImageRef;
use docsai_model::presentation::Shape;
use docsai_model::sheet::Sheet;
use docsai_model::text::{Block, ListItem, Section, TableRow};
use docsai_model::{ConversionReport, Document};

/// Content parsed for one kind of sequence.
#[derive(Debug)]
pub enum Fragment {
    Sections(Vec<Section>),
    Blocks(Vec<Block>),
    Rows(Vec<TableRow>),
    ListItems(Vec<ListItem>),
    Sheets(Vec<Sheet>),
    Slides(Vec<Slide>),
    Shapes(Vec<Shape>),
    Images(Vec<ImageRef>),
}

use docsai_model::presentation::Slide;

impl Fragment {
    pub fn kind(&self) -> SeqKind {
        match self {
            Fragment::Sections(_) => SeqKind::Sections,
            Fragment::Blocks(_) => SeqKind::Blocks,
            Fragment::Rows(_) => SeqKind::Rows,
            Fragment::ListItems(_) => SeqKind::ListItems,
            Fragment::Sheets(_) => SeqKind::Sheets,
            Fragment::Slides(_) => SeqKind::Slides,
            Fragment::Shapes(_) => SeqKind::Shapes,
            Fragment::Images(_) => SeqKind::Images,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Fragment::Sections(v) => v.len(),
            Fragment::Blocks(v) => v.len(),
            Fragment::Rows(v) => v.len(),
            Fragment::ListItems(v) => v.len(),
            Fragment::Sheets(v) => v.len(),
            Fragment::Slides(v) => v.len(),
            Fragment::Shapes(v) => v.len(),
            Fragment::Images(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why a fragment is not what the sequence holds.
#[derive(Debug)]
pub enum FragmentError {
    /// The DocMark did not parse. Carried through unchanged (§6.3).
    Parse(docsai_docmark::ParseError),
    /// It parsed, and it is not the kind the sequence takes.
    Mismatch {
        /// What the content turned out to be, in the words a refusal uses.
        got: String,
    },
    /// The sequence takes something no fragment can express, and §9 is the way
    /// round. A table's cells are the case: a column is not a node in the IR
    /// (§12.7), so there is nothing to parse into one.
    NoFragmentKind { reason: &'static str },
}

/// Parses `content` as the kind `into` holds.
pub fn parse_for(
    into: SeqKind,
    content: &str,
    base: Option<&Path>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Fragment, FragmentError> {
    match into {
        SeqKind::Slides => {
            // «There is no coercion» (§6.3). A deck body whose content precedes
            // any heading parses as *a slide with no title*, which is the right
            // reading for a whole document and a coercion for a fragment: a
            // caller that wrote a paragraph asked for a paragraph. So a slide
            // fragment must open a slide.
            if !opens_with_heading(content) {
                return Err(FragmentError::Mismatch {
                    got: "blocks".into(),
                });
            }
            let doc = parse(content, base, assets, report, Prefix::Deck)?;
            match doc {
                Document::Presentation(deck) => Ok(Fragment::Slides(deck.slides)),
                other => Err(mismatch(&other)),
            }
        }
        SeqKind::Shapes => {
            // A deck fragment whose content precedes any heading is «a slide
            // with no title», so the shapes are the slide's and no wrapper
            // heading has to be invented — and therefore no title placeholder
            // is fabricated for a shape the caller did not write.
            if opens_with_heading(content) {
                return Err(FragmentError::Mismatch {
                    got: "slides".into(),
                });
            }
            let doc = parse(content, base, assets, report, Prefix::Deck)?;
            match doc {
                Document::Presentation(mut deck) if deck.slides.len() == 1 => {
                    Ok(Fragment::Shapes(deck.slides.remove(0).shapes))
                }
                Document::Presentation(_) => Err(FragmentError::Mismatch {
                    got: "slides".into(),
                }),
                other => Err(mismatch(&other)),
            }
        }
        SeqKind::Sheets => {
            let doc = parse(content, base, assets, report, Prefix::Workbook)?;
            match doc {
                Document::Workbook(book) => Ok(Fragment::Sheets(book.sheets)),
                other => Err(mismatch(&other)),
            }
        }
        SeqKind::Sections => match parse(content, base, assets, report, Prefix::None)? {
            Document::Text(text) => Ok(Fragment::Sections(text.sections)),
            other => Err(mismatch(&other)),
        },
        SeqKind::Blocks => match parse(content, base, assets, report, Prefix::None)? {
            Document::Text(text) => Ok(Fragment::Blocks(flatten(text.sections))),
            other => Err(mismatch(&other)),
        },
        SeqKind::Rows => match parse(content, base, assets, report, Prefix::None)? {
            Document::Text(text) => match flatten(text.sections).as_mut_slice() {
                [Block::Table(table)] => Ok(Fragment::Rows(std::mem::take(&mut table.rows))),
                _ => Err(FragmentError::Mismatch {
                    got: "not a single table".into(),
                }),
            },
            other => Err(mismatch(&other)),
        },
        SeqKind::ListItems => match parse(content, base, assets, report, Prefix::None)? {
            Document::Text(text) => match flatten(text.sections).as_mut_slice() {
                [Block::List(list)] => Ok(Fragment::ListItems(std::mem::take(&mut list.items))),
                _ => Err(FragmentError::Mismatch {
                    got: "not a single list".into(),
                }),
            },
            other => Err(mismatch(&other)),
        },
        SeqKind::Images => match parse(content, base, assets, report, Prefix::None)? {
            Document::Text(text) => {
                let blocks = flatten(text.sections);
                let mut images = Vec::with_capacity(blocks.len());
                for block in blocks {
                    match block {
                        Block::Image(image) => images.push(image),
                        _ => {
                            return Err(FragmentError::Mismatch {
                                got: "blocks, not pictures".into(),
                            })
                        }
                    }
                }
                Ok(Fragment::Images(images))
            }
            other => Err(mismatch(&other)),
        },
        SeqKind::Cells => Err(FragmentError::NoFragmentKind {
            reason: "a table's columns are not nodes in the IR (§12.7), so a cell cannot be \
                     inserted; rewrite the table with `read --select` (§9)",
        }),
        SeqKind::Inlines => Err(FragmentError::NoFragmentKind {
            reason: "inline structure is not inserted by an operation (§12.9); it arrives inside \
                     a block-level fragment or through `read --select` (§9)",
        }),
        SeqKind::Comments => Err(FragmentError::NoFragmentKind {
            reason: "nothing in version 1 creates a comment: `docsai` cannot mint the assertion \
                     that a named person said something (§6.10)",
        }),
    }
}

/// Which dialect the fragment is read in.
///
/// DocMark chooses its dialect from the document, and a fragment has none, so
/// the sequence lends it one: a slide fragment is read as DocMark-P because it
/// is going into a deck, not because it happens to carry `{.slide}`.
#[derive(Clone, Copy)]
enum Prefix {
    None,
    Deck,
    Workbook,
}

impl Prefix {
    fn front_matter(self) -> &'static str {
        match self {
            Prefix::None => "",
            Prefix::Deck => "---\nsource-format: pptx\n---\n",
            Prefix::Workbook => "---\nsource-format: xlsx\n---\n",
        }
    }
}

fn parse(
    content: &str,
    base: Option<&Path>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
    prefix: Prefix,
) -> Result<Document, FragmentError> {
    let text = format!("{}{}", prefix.front_matter(), content);
    let (doc, parse_report) =
        docsai_docmark::parse_with_base(&text, base, assets).map_err(FragmentError::Parse)?;
    report.merge(parse_report);
    Ok(doc)
}

/// Whether the first thing the fragment says is a top-level heading, which is
/// what opens a slide in DocMark-P.
fn opens_with_heading(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| {
            let hashes = line.len() - line.trim_start_matches('#').len();
            (1..=2).contains(&hashes)
        })
}

fn flatten(sections: Vec<Section>) -> Vec<Block> {
    sections.into_iter().flat_map(|s| s.blocks).collect()
}

fn mismatch(doc: &Document) -> FragmentError {
    FragmentError::Mismatch {
        got: match doc {
            Document::Text(_) => "blocks".into(),
            Document::Workbook(_) => "sheets".into(),
            Document::Presentation(_) => "slides".into(),
        },
    }
}
