//! The case bodies — one function per capability, each answering `Ok` for a pass and `Err(the
//! difference)` for a failure.
//!
//! Split from `super` because the catalogue, the grading table and the capability probe already
//! fill a file. Nothing here is public: `super::run_conformance` is the only way in, and the same
//! source lint in `tests/adapter-conformance.rs` reads this file too.
//!
//! # Two rules every body here follows
//!
//! 1. **A case leans only on [`super::Run::can`], never on the declaration.** A capability the
//!    backend declared and then refused is already known to be unusable, so a case that needed it
//!    only to *observe* something reports `unsupported` rather than adding a second failure to one
//!    defect.
//! 2. **A case never grades another case's rule.** `OP-read/short-read-is-reported` deliberately
//!    does not compare against what `write` reported, because over-reporting a write is
//!    `OP-write/short-write-is-reported`'s defect and one planted fault must fail one case.

use super::*;

impl Run<'_> {
    pub(super) fn run_cases(&mut self, capability: AdapterCapability) {
        match capability {
            AdapterCapability::Lookup => self.lookup_cases(),
            AdapterCapability::Enumerate => self.enumerate_cases(),
            AdapterCapability::Open => self.open_cases(),
            AdapterCapability::Read => self.read_cases(),
            AdapterCapability::Write => self.write_cases(),
            AdapterCapability::SetFileLength => self.set_file_length_cases(),
            AdapterCapability::CreateFile => self.create_cases(),
            AdapterCapability::CreateDirectory => self.create_directory_cases(),
            AdapterCapability::Rename => self.rename_cases(),
            AdapterCapability::Move => self.move_cases(),
            AdapterCapability::Unlink => self.unlink_cases(),
            AdapterCapability::RemoveDirectory => self.remove_directory_cases(),
            AdapterCapability::ReadMetadata => self.read_metadata_cases(),
            AdapterCapability::WriteMetadata => self.write_metadata_cases(),
            AdapterCapability::MountActorView => self.mount_cases(),
            AdapterCapability::MaterializeReadonlyView => self.readonly_cases(),
            AdapterCapability::ObserveDurableBoundary => self.boundary_cases(),
            AdapterCapability::Symlink => self.skip(
                rules_for(AdapterCapability::Symlink),
                Some(AdapterCapability::Symlink),
                "Symlink is reserved and contract 0 publishes no operation for it",
            ),
        }
    }
}

impl<'a> Run<'a> {
    fn working_view(&self) -> Option<&'a dyn WorkspaceView> {
        self.resolve(self.actor_view)
    }

    fn lookup_cases(&mut self) {
        let subject = Some(AdapterCapability::Lookup);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Lookup), subject, NO_VIEW);
        };
        if self.can(AdapterCapability::CreateFile) {
            self.checked(&LOOKUP_RESOLVES, subject, || {
                let name = entry_name("lookup-resolves.txt");
                let created = expect_ok(
                    "create_file",
                    view.create_file(view.root(), &name, EXECUTABLE),
                )?;
                let found = expect_ok("lookup", view.lookup(view.root(), &name))?;
                if found.object() != created.object() {
                    return Err(format!(
                        "create_file bound {:?} and lookup resolved {:?}",
                        created.object(),
                        found.object()
                    ));
                }
                if found.name() != &name {
                    return Err(format!("lookup answered the entry named {}", found.name()));
                }
                if found.kind() != ObjectKind::File {
                    return Err(format!(
                        "a created file is reported as {}",
                        found.kind().as_str()
                    ));
                }
                if found.metadata() != EXECUTABLE {
                    return Err(
                        "the metadata create_file was given did not survive lookup".to_owned()
                    );
                }
                Ok(())
            });
        } else {
            self.emit(
                &LOOKUP_RESOLVES,
                subject,
                CaseResult::Unsupported,
                "CreateFile is not declared, so nothing can be bound for lookup to resolve"
                    .to_owned(),
            );
        }
        self.checked(&LOOKUP_MISSING, subject, || {
            expect_error(
                "lookup of an unbound name",
                view.lookup(view.root(), &entry_name("lookup-absent.txt")),
                &AdapterError::NotFound,
            )
        });
    }

    fn enumerate_cases(&mut self) {
        let subject = Some(AdapterCapability::Enumerate);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Enumerate), subject, NO_VIEW);
        };
        let can_create = self.can(AdapterCapability::CreateFile);
        self.checked(&ENUMERATE_IS_SORTED, subject, || {
            // Deliberately not in order: a backend that returns insertion order is caught here.
            if can_create {
                for text in ["ord-b", "ord-A", "ord-a"] {
                    expect_ok(
                        "create_file",
                        view.create_file(
                            view.root(),
                            &entry_name(text),
                            PortableMetadata::default(),
                        ),
                    )?;
                }
            }
            let entries = expect_ok("enumerate", view.enumerate(view.root()))?;
            let names: Vec<&str> = entries.iter().map(|entry| entry.name().as_str()).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            if names != sorted {
                return Err(format!(
                    "enumerate answered {names:?}; byte-lexicographic order is {sorted:?}"
                ));
            }
            if can_create {
                for text in ["ord-A", "ord-a", "ord-b"] {
                    if !names.contains(&text) {
                        return Err(format!(
                            "enumerate does not list {text}, which was created in this directory"
                        ));
                    }
                }
            }
            Ok(())
        });
        if can_create {
            self.checked(&ENUMERATE_A_FILE, subject, || {
                let created = expect_ok(
                    "create_file",
                    view.create_file(
                        view.root(),
                        &entry_name("ord-not-a-directory.txt"),
                        PortableMetadata::default(),
                    ),
                )?;
                expect_error(
                    "enumerate of a file",
                    view.enumerate(created.object()),
                    &AdapterError::NotADirectory,
                )
            });
        } else {
            self.emit(
                &ENUMERATE_A_FILE,
                subject,
                CaseResult::Unsupported,
                "CreateFile is not declared, so there is no file to address a directory operation to"
                    .to_owned(),
            );
        }
        self.checked(&ENUMERATE_REPEATS, subject, || {
            let first = expect_ok("enumerate", view.enumerate(view.root()))?;
            let second = expect_ok("enumerate", view.enumerate(view.root()))?;
            if first == second {
                Ok(())
            } else {
                Err(format!(
                    "two enumerations of an unchanged directory answered {} and {} entries",
                    first.len(),
                    second.len()
                ))
            }
        });
    }

    fn open_cases(&mut self) {
        let subject = Some(AdapterCapability::Open);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Open), subject, NO_VIEW);
        };
        if self.can(AdapterCapability::CreateFile) {
            self.checked(&OPEN_ROUND_TRIPS, subject, || {
                let name = entry_name("open-round-trip.txt");
                let created = expect_ok(
                    "create_file",
                    view.create_file(view.root(), &name, PortableMetadata::default()),
                )?;
                let handle = expect_ok("open", view.open(created.object(), OpenMode::ReadWrite))?;
                if handle.object() != created.object() {
                    return Err("the handle names an object other than the one opened".to_owned());
                }
                if handle.view() != view.id() {
                    return Err(
                        "the handle names a view other than the one it was taken in".to_owned()
                    );
                }
                if handle.mode() != OpenMode::ReadWrite {
                    return Err(format!("the handle reports mode {:?}", handle.mode()));
                }
                expect_ok("close", view.close(handle.clone()))?;
                expect_error(
                    "a second close",
                    view.close(handle),
                    &AdapterError::NotFound,
                )
            });
        } else {
            self.emit(
                &OPEN_ROUND_TRIPS,
                subject,
                CaseResult::Unsupported,
                "CreateFile is not declared, so there is no file to open".to_owned(),
            );
        }
        self.checked(&OPEN_A_DIRECTORY, subject, || {
            expect_error(
                "open of a directory",
                view.open(view.root(), OpenMode::Read),
                &AdapterError::IsADirectory,
            )
        });
    }

    fn read_cases(&mut self) {
        let subject = Some(AdapterCapability::Read);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Read), subject, NO_VIEW);
        };
        if !(self.can(AdapterCapability::CreateFile) && self.can(AdapterCapability::Open)) {
            return self.emit(
                &READ_IS_REPORTED,
                subject,
                CaseResult::Unsupported,
                "CreateFile and Open are both needed to reach a readable handle".to_owned(),
            );
        }
        let can_write = self.can(AdapterCapability::Write);
        self.checked(&READ_IS_REPORTED, subject, || {
            let name = entry_name("read-reported.txt");
            let created = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, PortableMetadata::default()),
            )?;
            let handle = expect_ok("open", view.open(created.object(), OpenMode::ReadWrite))?;
            if can_write {
                // The count this write reports is deliberately NOT the expectation: a write that
                // over-reports is `OP-write/short-write-is-reported`'s defect, and grading it here
                // too would make one planted fault fail two cases.
                expect_ok("write", view.write(&handle, 0, b"0123456789"))?;
            }
            let mut buffer = [0u8; 32];
            let whole = expect_ok("read", view.read(&handle, 0, &mut buffer))?;
            if whole > buffer.len() {
                return Err(format!(
                    "read answered {whole} into a {}-byte buffer",
                    buffer.len()
                ));
            }
            let at_the_end = expect_ok(
                "read at the end",
                view.read(&handle, whole as u64, &mut buffer),
            )?;
            if at_the_end != 0 {
                return Err(format!(
                    "the file holds {whole} readable bytes and reading from offset {whole} answered {at_the_end} rather than 0. A read that answers the buffer length regardless is a read nobody can trust"
                ));
            }
            let midpoint = whole / 2;
            let tail = expect_ok(
                "read from the midpoint",
                view.read(&handle, midpoint as u64, &mut buffer),
            )?;
            if tail != whole - midpoint {
                return Err(format!(
                    "the file holds {whole} readable bytes and reading from offset {midpoint} answered {tail} rather than {}",
                    whole - midpoint
                ));
            }
            expect_ok("close", view.close(handle))
        });
    }

    fn write_cases(&mut self) {
        let subject = Some(AdapterCapability::Write);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Write), subject, NO_VIEW);
        };
        if !(self.can(AdapterCapability::CreateFile)
            && self.can(AdapterCapability::Open)
            && self.can(AdapterCapability::Read))
        {
            return self.emit(
                &WRITE_IS_REPORTED,
                subject,
                CaseResult::Unsupported,
                "CreateFile, Open and Read are all needed to check that what a write reported is what it stored".to_owned(),
            );
        }
        self.checked(&WRITE_IS_REPORTED, subject, || {
            const PAYLOAD: &[u8] = b"0123456789";
            let name = entry_name("write-reported.txt");
            let created = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, PortableMetadata::default()),
            )?;
            let handle = expect_ok("open", view.open(created.object(), OpenMode::ReadWrite))?;
            let taken = expect_ok("write", view.write(&handle, 0, PAYLOAD))?;
            if taken > PAYLOAD.len() {
                return Err(format!(
                    "write was handed {} bytes and reported taking {taken}",
                    PAYLOAD.len()
                ));
            }
            let mut buffer = [0u8; 32];
            let filled = expect_ok("read", view.read(&handle, 0, &mut buffer))?;
            if filled != taken {
                return Err(format!(
                    "write reported taking {taken} bytes and {filled} are readable. A write that reports more than it stored is data loss"
                ));
            }
            if buffer[..filled] != PAYLOAD[..taken] {
                return Err("the bytes that came back are not the bytes that went in".to_owned());
            }
            expect_ok("close", view.close(handle))
        });
    }

    fn set_file_length_cases(&mut self) {
        let subject = Some(AdapterCapability::SetFileLength);
        let Some(view) = self.working_view() else {
            return self.skip(
                rules_for(AdapterCapability::SetFileLength),
                subject,
                NO_VIEW,
            );
        };
        let can_observe = self.can(AdapterCapability::CreateFile)
            && self.can(AdapterCapability::Open)
            && self.can(AdapterCapability::Read)
            && self.can(AdapterCapability::Write);
        if can_observe {
            self.checked(&SET_LENGTH_SHRINKS, subject, || {
                let name = entry_name("length-shrink.txt");
                let created = expect_ok(
                    "create_file",
                    view.create_file(view.root(), &name, PortableMetadata::default()),
                )?;
                let handle = expect_ok("open", view.open(created.object(), OpenMode::ReadWrite))?;
                expect_ok(
                    "write",
                    view.write(&handle, 0, b"a much longer first draft"),
                )?;
                expect_ok("set_file_length", view.set_file_length(created.object(), 5))?;
                let mut buffer = [0u8; 32];
                let filled = expect_ok("read", view.read(&handle, 0, &mut buffer))?;
                if filled != 5 || &buffer[..filled] != b"a muc" {
                    return Err(format!(
                        "shortening to 5 bytes returned {filled} bytes: {:?}",
                        &buffer[..filled.min(buffer.len())]
                    ));
                }
                let tail = expect_ok("read at the new end", view.read(&handle, 5, &mut buffer))?;
                if tail != 0 {
                    return Err(format!(
                        "{tail} stale bytes remain readable after the new end"
                    ));
                }
                expect_ok("close", view.close(handle))
            });

            self.checked(&SET_LENGTH_GROWS_WITH_ZEROES, subject, || {
                let name = entry_name("length-grow.txt");
                let created = expect_ok(
                    "create_file",
                    view.create_file(view.root(), &name, PortableMetadata::default()),
                )?;
                let handle = expect_ok("open", view.open(created.object(), OpenMode::ReadWrite))?;
                let written = expect_ok("write", view.write(&handle, 0, b"abc"))?;
                if written != 3 {
                    return Err(format!(
                        "write reported taking {written} bytes while preparing the growth oracle"
                    ));
                }
                expect_ok("set_file_length", view.set_file_length(created.object(), 7))?;
                let mut buffer = [0xff; 16];
                let filled = expect_ok("read", view.read(&handle, 0, &mut buffer))?;
                if filled != 7 || &buffer[..filled] != b"abc\0\0\0\0" {
                    return Err(format!(
                        "growing a three-byte nonzero file to 7 returned {filled} bytes: {:?}",
                        &buffer[..filled.min(buffer.len())]
                    ));
                }
                expect_ok("close", view.close(handle))
            });

            self.checked(&SET_LENGTH_ZERO_IS_EMPTY, subject, || {
                let name = entry_name("length-zero.txt");
                let created = expect_ok(
                    "create_file",
                    view.create_file(view.root(), &name, PortableMetadata::default()),
                )?;
                let handle = expect_ok("open", view.open(created.object(), OpenMode::ReadWrite))?;
                expect_ok("write", view.write(&handle, 0, b"not empty"))?;
                expect_ok("set_file_length", view.set_file_length(created.object(), 0))?;
                let mut buffer = [0u8; 1];
                let filled = expect_ok("read", view.read(&handle, 0, &mut buffer))?;
                if filled != 0 {
                    return Err(format!("setting length zero left {filled} byte readable"));
                }
                expect_ok("close", view.close(handle))
            });
        } else {
            self.skip(
                &[
                    SET_LENGTH_SHRINKS,
                    SET_LENGTH_GROWS_WITH_ZEROES,
                    SET_LENGTH_ZERO_IS_EMPTY,
                ],
                subject,
                "CreateFile, Open, Read and Write are needed to observe exact file length changes",
            );
        }

        self.checked(&SET_LENGTH_DIRECTORY_IS_REFUSED, subject, || {
            expect_error(
                "set_file_length on a directory",
                view.set_file_length(view.root(), 0),
                &AdapterError::IsADirectory,
            )
        });
        self.checked(&SET_LENGTH_MISSING_IS_REFUSED, subject, || {
            expect_error(
                "set_file_length on an unknown object",
                view.set_file_length(ObjectId::from_bytes([0xff; 16]), 0),
                &AdapterError::NotFound,
            )
        });
    }

    fn create_cases(&mut self) {
        let subject = Some(AdapterCapability::CreateFile);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::CreateFile), subject, NO_VIEW);
        };
        let can_look_up = self.can(AdapterCapability::Lookup);
        self.checked(&CREATE_NAME_TAKEN, subject, || {
            let name = entry_name("create-taken.txt");
            let first = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, EXECUTABLE),
            )?;
            expect_error(
                "a second create_file over the same name",
                view.create_file(view.root(), &name, PortableMetadata::default()),
                &AdapterError::AlreadyExists,
            )?;
            if can_look_up {
                let found = expect_ok("lookup", view.lookup(view.root(), &name))?;
                if found.object() != first.object() {
                    return Err(
                        "the refused create replaced the entry that was already there".to_owned(),
                    );
                }
                if found.metadata() != EXECUTABLE {
                    return Err(
                        "the refused create overwrote the existing entry's metadata".to_owned()
                    );
                }
            }
            Ok(())
        });
        self.checked(&CREATE_IS_VISIBLE, subject, || {
            let name = entry_name("create-visible.txt");
            let created = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, EXECUTABLE),
            )?;
            if created.kind() != ObjectKind::File {
                return Err(format!(
                    "create_file answered an entry of kind {}",
                    created.kind().as_str()
                ));
            }
            if created.name() != &name {
                return Err(format!(
                    "create_file was given {name} and answered an entry named {}",
                    created.name()
                ));
            }
            if created.metadata() != EXECUTABLE {
                return Err("create_file did not keep the metadata it was given".to_owned());
            }
            Ok(())
        });
    }

    fn create_directory_cases(&mut self) {
        let subject = Some(AdapterCapability::CreateDirectory);
        let Some(view) = self.working_view() else {
            return self.skip(
                rules_for(AdapterCapability::CreateDirectory),
                subject,
                NO_VIEW,
            );
        };
        let can_enumerate = self.can(AdapterCapability::Enumerate);
        self.checked(&CREATE_DIRECTORY_IS_EMPTY, subject, || {
            let name = entry_name("createdir-empty");
            let created = expect_ok(
                "create_directory",
                view.create_directory(view.root(), &name),
            )?;
            if created.kind() != ObjectKind::Directory {
                return Err(format!(
                    "create_directory answered an entry of kind {}",
                    created.kind().as_str()
                ));
            }
            if can_enumerate {
                let entries = expect_ok("enumerate", view.enumerate(created.object()))?;
                if !entries.is_empty() {
                    return Err(format!(
                        "a new directory already holds {} entries",
                        entries.len()
                    ));
                }
            }
            expect_error(
                "a second create_directory over the same name",
                view.create_directory(view.root(), &name),
                &AdapterError::AlreadyExists,
            )
        });
    }

    fn rename_cases(&mut self) {
        let subject = Some(AdapterCapability::Rename);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Rename), subject, NO_VIEW);
        };
        if !(self.can(AdapterCapability::CreateFile) && self.can(AdapterCapability::Lookup)) {
            return self.skip(
                rules_for(AdapterCapability::Rename),
                subject,
                "CreateFile and Lookup are both needed to observe what a rename did",
            );
        }
        self.checked(&RENAME_KEEPS_METADATA, subject, || {
            let from = entry_name("rename-meta-from.txt");
            let to = entry_name("rename-meta-to.txt");
            let created =
                expect_ok("create_file", view.create_file(view.root(), &from, EXECUTABLE))?;
            expect_ok("rename", view.rename(view.root(), &from, &to))?;
            let found = expect_ok("lookup after rename", view.lookup(view.root(), &to))?;
            if found.object() != created.object() {
                return Err("rename changed the object the name is bound to".to_owned());
            }
            if found.metadata() != EXECUTABLE {
                return Err(
                    "the portable metadata did not survive the rename; a rename changes the name and nothing else"
                        .to_owned(),
                );
            }
            Ok(())
        });
        self.checked(&RENAME_CLEARS_THE_OLD_NAME, subject, || {
            let from = entry_name("rename-old-from.txt");
            let to = entry_name("rename-old-to.txt");
            expect_ok(
                "create_file",
                view.create_file(view.root(), &from, PortableMetadata::default()),
            )?;
            expect_ok("rename", view.rename(view.root(), &from, &to))?;
            expect_error(
                "lookup of the old name",
                view.lookup(view.root(), &from),
                &AdapterError::NotFound,
            )?;
            expect_ok("lookup of the new name", view.lookup(view.root(), &to))?;
            Ok(())
        });
    }

    fn move_cases(&mut self) {
        let subject = Some(AdapterCapability::Move);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Move), subject, NO_VIEW);
        };
        if !(self.can(AdapterCapability::CreateDirectory)
            && self.can(AdapterCapability::CreateFile)
            && self.can(AdapterCapability::Lookup))
        {
            return self.skip(
                rules_for(AdapterCapability::Move),
                subject,
                "CreateDirectory, CreateFile and Lookup are all needed to build two directories and observe the move",
            );
        }
        self.checked(&MOVE_REFUSES_A_CYCLE, subject, || {
            let outer = entry_name("move-outer");
            let inner = entry_name("move-inner");
            let outer_entry = expect_ok(
                "create_directory",
                view.create_directory(view.root(), &outer),
            )?;
            let inner_entry = expect_ok(
                "create_directory",
                view.create_directory(outer_entry.object(), &inner),
            )?;
            expect_error(
                "moving a directory inside its own subtree",
                view.move_entry(view.root(), &outer, inner_entry.object(), &outer),
                &AdapterError::WouldCycle,
            )?;
            let still_there = expect_ok("lookup", view.lookup(view.root(), &outer))?;
            if still_there.object() != outer_entry.object() {
                return Err("the refused move changed the tree anyway".to_owned());
            }
            Ok(())
        });
        self.checked(&MOVE_KEEPS_METADATA, subject, || {
            let destination = entry_name("move-destination");
            let name = entry_name("move-me.txt");
            let destination_entry = expect_ok(
                "create_directory",
                view.create_directory(view.root(), &destination),
            )?;
            let created = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, EXECUTABLE),
            )?;
            expect_ok(
                "move_entry",
                view.move_entry(view.root(), &name, destination_entry.object(), &name),
            )?;
            let found = expect_ok(
                "lookup in the destination",
                view.lookup(destination_entry.object(), &name),
            )?;
            if found.object() != created.object() {
                return Err("the move changed the object the name is bound to".to_owned());
            }
            if found.metadata() != EXECUTABLE {
                return Err("the portable metadata did not survive the move".to_owned());
            }
            expect_error(
                "lookup of the old binding",
                view.lookup(view.root(), &name),
                &AdapterError::NotFound,
            )
        });
    }

    fn unlink_cases(&mut self) {
        let subject = Some(AdapterCapability::Unlink);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::Unlink), subject, NO_VIEW);
        };
        if !(self.can(AdapterCapability::CreateFile) && self.can(AdapterCapability::Lookup)) {
            return self.skip(
                rules_for(AdapterCapability::Unlink),
                subject,
                "CreateFile and Lookup are both needed to observe that an entry is gone",
            );
        }
        let can_enumerate = self.can(AdapterCapability::Enumerate);
        self.checked(&UNLINK_ENTRY_IS_GONE, subject, || {
            let name = entry_name("unlink-me.txt");
            expect_ok(
                "create_file",
                view.create_file(view.root(), &name, PortableMetadata::default()),
            )?;
            expect_ok("lookup before unlink", view.lookup(view.root(), &name))?;
            expect_ok("unlink", view.unlink(view.root(), &name))?;
            expect_error(
                "lookup after unlink",
                view.lookup(view.root(), &name),
                &AdapterError::NotFound,
            )?;
            if can_enumerate {
                let entries = expect_ok("enumerate", view.enumerate(view.root()))?;
                if entries
                    .iter()
                    .any(|entry| entry.name().as_str() == name.as_str())
                {
                    return Err("the directory still lists a name that was unlinked".to_owned());
                }
            }
            Ok(())
        });
    }

    fn remove_directory_cases(&mut self) {
        let subject = Some(AdapterCapability::RemoveDirectory);
        let Some(view) = self.working_view() else {
            return self.skip(
                rules_for(AdapterCapability::RemoveDirectory),
                subject,
                NO_VIEW,
            );
        };
        if !(self.can(AdapterCapability::CreateDirectory)
            && self.can(AdapterCapability::CreateFile)
            && self.can(AdapterCapability::Lookup))
        {
            return self.skip(
                rules_for(AdapterCapability::RemoveDirectory),
                subject,
                "CreateDirectory, CreateFile and Lookup are all needed to build a non-empty directory and observe what happened to it",
            );
        }
        self.checked(&RMDIR_NOT_EMPTY, subject, || {
            let directory = entry_name("rmdir-full");
            let kept = entry_name("rmdir-kept.txt");
            let entry = expect_ok(
                "create_directory",
                view.create_directory(view.root(), &directory),
            )?;
            expect_ok(
                "create_file",
                view.create_file(entry.object(), &kept, PortableMetadata::default()),
            )?;
            expect_error(
                "remove_directory on a directory that still has entries",
                view.remove_directory(view.root(), &directory),
                &AdapterError::DirectoryNotEmpty,
            )?;
            expect_ok(
                "lookup of the directory afterwards",
                view.lookup(view.root(), &directory),
            )?;
            expect_ok(
                "lookup of the entry inside it",
                view.lookup(entry.object(), &kept),
            )?;
            Ok(())
        });
        self.checked(&RMDIR_EMPTY, subject, || {
            let directory = entry_name("rmdir-empty");
            expect_ok(
                "create_directory",
                view.create_directory(view.root(), &directory),
            )?;
            expect_ok(
                "remove_directory",
                view.remove_directory(view.root(), &directory),
            )?;
            expect_error(
                "lookup after remove_directory",
                view.lookup(view.root(), &directory),
                &AdapterError::NotFound,
            )
        });
    }

    fn read_metadata_cases(&mut self) {
        let subject = Some(AdapterCapability::ReadMetadata);
        let Some(view) = self.working_view() else {
            return self.skip(rules_for(AdapterCapability::ReadMetadata), subject, NO_VIEW);
        };
        if !self.can(AdapterCapability::CreateFile) {
            return self.skip(
                rules_for(AdapterCapability::ReadMetadata),
                subject,
                "CreateFile is needed to have an object whose metadata is known",
            );
        }
        self.checked(&METADATA_REPORTS, subject, || {
            let name = entry_name("metadata-reports.txt");
            let created = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, EXECUTABLE),
            )?;
            let reported = expect_ok("metadata", view.metadata(created.object()))?;
            if reported == EXECUTABLE {
                Ok(())
            } else {
                Err(format!(
                    "the file was created executable and metadata answers executable={}",
                    reported.is_executable()
                ))
            }
        });
    }

    fn write_metadata_cases(&mut self) {
        let subject = Some(AdapterCapability::WriteMetadata);
        let Some(view) = self.working_view() else {
            return self.skip(
                rules_for(AdapterCapability::WriteMetadata),
                subject,
                NO_VIEW,
            );
        };
        if !self.can(AdapterCapability::CreateFile) {
            return self.skip(
                rules_for(AdapterCapability::WriteMetadata),
                subject,
                "CreateFile is needed to have an object to set metadata on",
            );
        }
        let can_read_metadata = self.can(AdapterCapability::ReadMetadata);
        let can_look_up = self.can(AdapterCapability::Lookup);
        self.checked(&SET_METADATA_IS_VISIBLE, subject, || {
            let name = entry_name("metadata-set.txt");
            let created = expect_ok(
                "create_file",
                view.create_file(view.root(), &name, PortableMetadata::default()),
            )?;
            expect_ok(
                "set_metadata",
                view.set_metadata(created.object(), EXECUTABLE),
            )?;
            if can_read_metadata {
                let reported = expect_ok("metadata", view.metadata(created.object()))?;
                if reported != EXECUTABLE {
                    return Err("set_metadata is not visible to metadata".to_owned());
                }
            }
            if can_look_up {
                let found = expect_ok("lookup", view.lookup(view.root(), &name))?;
                if found.metadata() != EXECUTABLE {
                    return Err("set_metadata is not visible on the directory entry".to_owned());
                }
            }
            Ok(())
        });
    }

    fn mount_cases(&mut self) {
        let subject = Some(AdapterCapability::MountActorView);
        let adapter = self.adapter;
        let workspace = self.fixture.workspace();
        let unprepared = self.preparation_note();
        let (first, second, released) = (self.actor_view, self.second_view, self.released_view);

        self.checked(&TWO_VIEWS_COEXIST, subject, || {
            let (Some(first), Some(second)) = (first, second) else {
                return Err(format!(
                    "MountActorView is declared and two actors could not both be mounted in the workspace prepare_fixture() named.{unprepared}"
                ));
            };
            if first == second {
                return Err(format!(
                    "two actors mounted and both were given {first}; a view identifier names one view"
                ));
            }
            let one = adapter
                .view(first)
                .map_err(|error| format!("the first view no longer resolves: {error}"))?;
            let other = adapter
                .view(second)
                .map_err(|error| format!("the second view no longer resolves: {error}"))?;
            if one.id() != first || other.id() != second {
                return Err("a view reports an identifier other than the one it resolved from".to_owned());
            }
            if one.access() != ViewAccess::ReadWrite || other.access() != ViewAccess::ReadWrite {
                return Err("a mounted actor view is not read-write; an actor would be unable to work in their own state".to_owned());
            }
            Ok(())
        });

        let note = unprepared.clone();
        self.checked(&RELEASED_VIEW_IS_UNKNOWN, subject, || {
            let Some(released) = released else {
                return Err(format!(
                    "MountActorView is declared and a third view could not be mounted to release.{note}"
                ));
            };
            adapter
                .release(released)
                .map_err(|error| format!("releasing a view that was just mounted answered {error}"))?;
            match adapter.view(released) {
                Err(AdapterError::UnknownView) => {}
                Err(other) => return Err(format!("a released view answered {}", other.name())),
                Ok(_) => {
                    return Err(
                        "a released view identifier still resolves, so a caller can keep using a view it gave up"
                            .to_owned(),
                    )
                }
            }
            expect_error(
                "a second release",
                adapter.release(released),
                &AdapterError::UnknownView,
            )
        });

        self.checked(&RELATIVE_NAMES_ARE_REFUSED, subject, || {
            expect_error(
                "mounting at a path holding a `..` component",
                adapter.mount_actor_view(
                    workspace,
                    FIXTURE_ACTOR_RELATIVE,
                    Path::new(MOUNTPOINT_RELATIVE),
                ),
                &AdapterError::NameRejected(NameError::Relative),
            )
        });
    }

    fn readonly_cases(&mut self) {
        let subject = Some(AdapterCapability::MaterializeReadonlyView);
        let adapter = self.adapter;
        let head = self.fixture.head();
        let Some(view) = self.resolve(self.readonly_view) else {
            // `fail`, not `unsupported`. This body only runs for a backend that *declared*
            // MaterializeReadonlyView, and `unsupported` is the word an honest partial backend
            // earns; spending it on a declared capability that could not be reached would let a
            // backend be conformant with nothing about read-only refusal checked at all.
            // `NAME/relative-target-is-refused` is not among these: it hands the backend a path and
            // needs no view, so it still runs below.
            let detail = format!(
                "MaterializeReadonlyView is declared and no read-only view could be obtained for the head prepare_fixture() named, so nothing about a read-only view was graded.{}",
                self.preparation_note()
            );
            for rule in [
                &READONLY_ACCESS,
                &READONLY_WRITE_IS_REFUSED,
                &READONLY_EVERY_MUTATION,
            ] {
                self.emit(rule, subject, CaseResult::Fail, detail.clone());
            }
            return self.checked(&RELATIVE_TARGET_IS_REFUSED, subject, || {
                expect_error(
                    "presenting a read-only view at a path holding a `..` component",
                    adapter.materialize_readonly_view(head, Path::new(READONLY_RELATIVE_TARGET)),
                    &AdapterError::NameRejected(NameError::Relative),
                )
            });
        };

        self.checked(&READONLY_ACCESS, subject, || {
            if view.access() == ViewAccess::ReadOnly {
                Ok(())
            } else {
                Err("the view a read-only presentation answered reports ReadWrite".to_owned())
            }
        });

        if self.can(AdapterCapability::Write) {
            self.checked(&READONLY_WRITE_IS_REFUSED, subject, || {
                let handle = synthetic_handle(view.id(), view.root());
                expect_error(
                    "write on a read-only view",
                    view.write(&handle, 0, b"not mine to write"),
                    &AdapterError::ReadOnly,
                )
            });
        } else {
            self.emit(
                &READONLY_WRITE_IS_REFUSED,
                subject,
                CaseResult::Unsupported,
                "Write is not declared, so an undeclared capability's Unsupported is the right answer here and ReadOnly is not"
                    .to_owned(),
            );
        }

        let mutating: Vec<AdapterCapability> = [
            AdapterCapability::CreateFile,
            AdapterCapability::CreateDirectory,
            AdapterCapability::SetFileLength,
            AdapterCapability::Rename,
            AdapterCapability::Move,
            AdapterCapability::Unlink,
            AdapterCapability::RemoveDirectory,
            AdapterCapability::WriteMetadata,
        ]
        .into_iter()
        .filter(|capability| self.can(*capability))
        .collect();
        if mutating.is_empty() {
            self.emit(
                &READONLY_EVERY_MUTATION,
                subject,
                CaseResult::Unsupported,
                "no mutating capability is declared, so there is nothing a read-only view could refuse with ReadOnly"
                    .to_owned(),
            );
        } else {
            self.checked(&READONLY_EVERY_MUTATION, subject, || {
                let name = entry_name("readonly-attempt");
                let other = entry_name("readonly-attempt-other");
                let root = view.root();
                for capability in mutating {
                    match capability {
                        AdapterCapability::SetFileLength => expect_error(
                            "set_file_length on a read-only view",
                            view.set_file_length(root, 0),
                            &AdapterError::ReadOnly,
                        )?,
                        AdapterCapability::CreateFile => expect_error(
                            "create_file on a read-only view",
                            view.create_file(root, &name, PortableMetadata::default()),
                            &AdapterError::ReadOnly,
                        )?,
                        AdapterCapability::CreateDirectory => expect_error(
                            "create_directory on a read-only view",
                            view.create_directory(root, &name),
                            &AdapterError::ReadOnly,
                        )?,
                        AdapterCapability::Rename => expect_error(
                            "rename on a read-only view",
                            view.rename(root, &name, &other),
                            &AdapterError::ReadOnly,
                        )?,
                        AdapterCapability::Move => expect_error(
                            "move_entry on a read-only view",
                            view.move_entry(root, &name, root, &other),
                            &AdapterError::ReadOnly,
                        )?,
                        AdapterCapability::Unlink => expect_error(
                            "unlink on a read-only view",
                            view.unlink(root, &name),
                            &AdapterError::ReadOnly,
                        )?,
                        AdapterCapability::RemoveDirectory => expect_error(
                            "remove_directory on a read-only view",
                            view.remove_directory(root, &name),
                            &AdapterError::ReadOnly,
                        )?,
                        _ => expect_error(
                            "set_metadata on a read-only view",
                            view.set_metadata(root, EXECUTABLE),
                            &AdapterError::ReadOnly,
                        )?,
                    }
                }
                Ok(())
            });
        }

        self.checked(&RELATIVE_TARGET_IS_REFUSED, subject, || {
            expect_error(
                "presenting a read-only view at a path holding a `..` component",
                adapter.materialize_readonly_view(head, Path::new(READONLY_RELATIVE_TARGET)),
                &AdapterError::NameRejected(NameError::Relative),
            )
        });
    }

    fn boundary_cases(&mut self) {
        let subject = Some(AdapterCapability::ObserveDurableBoundary);
        let adapter = self.adapter;
        let view = self.actor_view.unwrap_or(ViewId::new(0));
        self.checked(&ONE_CLOSE_ONE_CANDIDATE, subject, || {
            let mut offered: Vec<CheckpointCandidate> = Vec::new();
            for event in boundary_stream(view) {
                match adapter.observe_durable_boundary(&event) {
                    Ok(Some(candidate)) => offered.push(candidate),
                    Ok(None) => {}
                    Err(AdapterError::UnknownView) => return Ok(()),
                    Err(other) => {
                        return Err(format!(
                            "observing a {} event answered {}",
                            event.kind().as_str(),
                            other.name()
                        ))
                    }
                }
            }
            if offered.len() > 1 {
                return Err(format!(
                    "one open, one write, one flush and one close offered {} candidates; one release of one handle offers at most one",
                    offered.len()
                ));
            }
            if let Some(candidate) = offered.first() {
                if candidate.view() != view {
                    return Err("the candidate names a view other than the one the events were in".to_owned());
                }
                let position = candidate.through().number();
                if !(1..=4).contains(&position) {
                    return Err(format!(
                        "the candidate covers position {position}, which is outside the stream it was shown"
                    ));
                }
            }
            Ok(())
        });

        self.checked(&REPLAY_IS_IDENTICAL, subject, || {
            let observe = || {
                boundary_stream(view)
                    .into_iter()
                    .map(|event| adapter.observe_durable_boundary(&event))
                    .collect::<Vec<_>>()
            };
            let first = observe();
            let second = observe();
            if first == second {
                Ok(())
            } else {
                Err(
                    "replaying one stream produced a different answer sequence, so boundary observation is a function of something other than the stream"
                        .to_owned(),
                )
            }
        });
    }
}
