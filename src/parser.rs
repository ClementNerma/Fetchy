use parsy::{char, choice, filter, just, newline, whitespaces, Parser};

use crate::{
    arch::{CpuArch, PlatformDependent, PlatformDependentEntry, System},
    pattern::Pattern,
    repository::{
        ArchiveFormat, DownloadSource, FileExtraction, FileNature, Package, Repository,
        SingleFileExtraction,
    },
    sources::{
        direct::DirectSourceParams,
        github::{GitHubSourceParams, GitHubVersionExtraction},
    },
};

// TODO: validate filenames (and sanitize package names)

pub fn repository() -> impl Parser<Repository> {
    let ms = whitespaces().no_newline();
    let msnl = whitespaces();
    let s = ms.at_least_one();

    let string = char('"')
        .ignore_then(
            filter(|c| c != '\n' && c != '\r' && c != '"')
                .repeated()
                .at_least(1)
                .collect_string()
                .critical("expected a string"),
        )
        .then_ignore(char('"').critical("expected a closing quote after the string"));

    let arrow = just("->").line_padded();

    let system = choice::<_, System>((
        just("linux").to(System::linux),
        just("windows").to(System::windows),
    ))
    .atomic_err("expected a valid system name");

    let cpu_arch = choice::<_, CpuArch>((
        just("x86_64").to(CpuArch::x86_64),
        just("aarch64").to(CpuArch::aarch64),
    ))
    .atomic_err("expected a valid CPU architecture");

    let platform = system
        .then_ignore(char(':').critical_with_no_message())
        .then(cpu_arch);

    let file_nature = choice::<_, FileNature>((
        just("binary")
            .ignore_then(s)
            .ignore_then(string.critical("expected a binary filename"))
            .map(|copy_as| FileNature::Binary { copy_as }),
        just("library")
            .ignore_then(s)
            .ignore_then(string.critical("expected a library filename"))
            .map(|name| FileNature::Library { name }),
        just("isolated_dir")
            .ignore_then(s)
            .ignore_then(string.critical("expected a filename"))
            .map(|name| FileNature::IsolatedDir { name }),
    ));

    let pattern = string.and_then_str(|string| Pattern::parse(&string));

    let single_file_extraction = pattern
        .then_ignore(arrow)
        .then(file_nature.critical("expected a file nature"))
        .map(|(relative_path, nature)| SingleFileExtraction {
            relative_path,
            nature,
        });

    let archive_format = choice::<_, ArchiveFormat>((
        just("archive:TarGz").to(ArchiveFormat::TarGz),
        just("archive:TarXz").to(ArchiveFormat::TarXz),
        just("archive:Zip").to(ArchiveFormat::Zip),
    ))
    .atomic_err("expected a valid archive format");

    let file_extraction = choice::<_, FileExtraction>((
        just("binary")
            .ignore_then(s)
            .ignore_then(string.critical("expected a binary filename"))
            .map(|copy_as| FileExtraction::Binary { copy_as }),
        archive_format
            .then_ignore(ms)
            .then_ignore(char('(').critical_with_no_message())
            .then(
                single_file_extraction
                    .padded_by(msnl)
                    .separated_by(char(','))
                    .at_least(1)
                    .critical("expected file extractions"),
            )
            .then_ignore(char(')').critical_with_no_message())
            .map(|(format, files)| FileExtraction::Archive { format, files }),
    ));

    let direct_asset = platform
        .then_ignore(arrow.critical_with_no_message())
        .then_ignore(ms)
        .then(string.critical("expected an URL"))
        .then_ignore(arrow.critical_with_no_message())
        .then(file_extraction.critical("expected a file extraction"))
        .map::<_, PlatformDependentEntry<(String, FileExtraction)>>(
            |(((system, cpu_arch), asset_pattern), file_extraction)| {
                PlatformDependentEntry::new(system, cpu_arch, (asset_pattern, file_extraction))
            },
        );

    let direct_source_params = just("version")
        .critical_with_no_message()
        .ignore_then(char('(').critical_with_no_message())
        .ignore_then(string.critical("expected a hardcoded version string"))
        .then_ignore(char(')').critical_with_no_message())
        .then_ignore(s)
        .then_ignore(char('(').critical_with_no_message())
        .then(
            direct_asset
                .padded_by(msnl)
                .separated_by(char(','))
                .at_least(1)
                .critical("expected at least 1 downloadable asset")
                .map(PlatformDependent::new),
        )
        .then_ignore(char(')').critical_with_no_message())
        .map(|(hardcoded_version, urls)| DirectSourceParams {
            urls,
            hardcoded_version,
        });

    let github_asset = platform
        .critical_with_no_message()
        .then_ignore(arrow)
        .then_ignore(ms)
        .then_ignore(just("asset").critical_with_no_message())
        .then_ignore(char('(').critical_with_no_message())
        .then(
            string
                .critical("expected an asset pattern")
                .and_then_str(|pattern| Pattern::parse(&pattern)),
        )
        .then_ignore(char(')').critical_with_no_message())
        .then_ignore(arrow)
        .then(file_extraction)
        .map::<_, PlatformDependentEntry<(Pattern, FileExtraction)>>(
            |(((system, cpu_arch), asset_pattern), file_extraction)| {
                PlatformDependentEntry::new(system, cpu_arch, (asset_pattern, file_extraction))
            },
        );

    let github_source_params = string
        .critical("expected a GitHub username")
        .then_ignore(s.critical_with_no_message())
        .then(string.critical("expected a repository name"))
        .then_ignore(s.critical_with_no_message())
        .then_ignore(just("version").critical_with_no_message())
        .then_ignore(char('(').critical_with_no_message())
        .then(
            choice::<_, GitHubVersionExtraction>((
                just("TagName").to(GitHubVersionExtraction::TagName),
                just("ReleaseTitle").to(GitHubVersionExtraction::ReleaseTitle),
            ))
            .atomic_err("expected a valid GitHub version extraction model"),
        )
        .then_ignore(char(')').critical_with_no_message())
        .then_ignore(ms)
        .then_ignore(char('(').critical_with_no_message())
        .then(
            github_asset
                .padded_by(msnl)
                .separated_by(char(','))
                .map(PlatformDependent),
        )
        .then_ignore(char(')').critical_with_no_message())
        .map(
            |(((author, repo_name), version), asset)| GitHubSourceParams {
                author,
                repo_name,
                version,
                asset,
            },
        );

    let package = string
        .then(
            s.ignore_then(just("(requires"))
                .ignore_then(s.critical_with_no_message())
                .ignore_then(
                    string
                        .separated_by(char(',').padded_by(ms))
                        .critical("expected a list of dependencies"),
                )
                .then_ignore(char(')').critical_with_no_message())
                .or_not(),
        )
        .then_ignore(char(':').critical_with_no_message())
        .then_ignore(msnl)
        .then(
            choice::<_, DownloadSource>((
                just("Direct")
                    .ignore_then(s)
                    .ignore_then(
                        direct_source_params
                            .critical("expected to find valid direct source parameters"),
                    )
                    .map(DownloadSource::Direct),
                just("GitHub")
                    .ignore_then(s)
                    .ignore_then(
                        github_source_params
                            .critical("expected to find valid GitHub source parameters"),
                    )
                    .map(DownloadSource::GitHub),
            ))
            .critical("expected a valid download source"),
        )
        .map(|((name, depends_on), download)| Package {
            name,
            depends_on,
            source: download,
        });

    let name = just("@name").ignore_then(s).ignore_then(string);

    let description = just("@description").ignore_then(s).ignore_then(string);

    let newlines = newline().repeated().at_least(1);

    let packages = just("@packages")
        .ignore_then(s)
        .ignore_then(char('(').critical_with_no_message())
        .ignore_then(
            package
                .padded_by(msnl)
                .repeated_vec()
                .at_least(1)
                .critical("expected at least 1 package in repository"),
        )
        .then_ignore(char(')').critical_with_no_message());

    let repository = name
        .critical("expected a repository name")
        .then_ignore(newlines.critical_with_no_message())
        .then(description.critical("expected a repository description"))
        .then_ignore(newlines.critical_with_no_message())
        .then(packages.critical("expected a list of packages"))
        .map(|((name, description), packages)| Repository {
            name,
            description,
            packages,
        });

    repository.padded_by(msnl).full()
}
