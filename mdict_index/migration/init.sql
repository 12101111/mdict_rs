CREATE TABLE meta (
    key text primary key not null,
    value text not null
);
CREATE TABLE mdx_block (
    block_index integer primary key not null,
    block_offset bigint not null,
    block_size bigint not null
);
CREATE TABLE mdx_index (
    keyword text primary key not null,
    block_index integer not null,
    record_offset integer not null,
    record_size integer not null,
    foreign key (block_index) references mdx_block(block_index)
);
CREATE TABLE mdd_block (
    file_index integer,
    block_index integer,
    block_offset bigint not null,
    block_size bigint not null,
    primary key (file_index, block_index)
);
CREATE TABLE mdd_index (
    keyword text primary key not null,
    file_index integer not null,
    block_index integer not null,
    record_offset integer not null,
    record_size integer not null,
    foreign key (file_index, block_index) references mdd_block(file_index, block_index)
);
