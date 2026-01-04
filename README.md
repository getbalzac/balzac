# Balzac

> an experiment in building a static site generator using rust

## Usage

- Install balzac
- Create a balzac.toml configuration file


## Directory Structure

Balzac supports different directories that you are free to create or skip:

- pages_directory (required): directory that stores all the handlebars templates that will be used to create pages
- partials_directory (optional): houses all handlebars partials
- layouts_directory (optional): houses all handlebars layouts



## Config Reference

- output_directory : the directory where the generated website will be sent to
- pages_directory (optional): the directory where static pages will reside (like your index)
- partials_directory (optional): directory where partial templates will reside
- layouts_directory (optional): directory where layout templates will reside
- assets_directory (optional): directory where static assets will reside
- content_directory (optional): directory where content (markdown) will reside
- global: fill this array if you want to have global data available in all the templates and files


## Collections

To use a collection, create a subfolder in the content_directory folder with the name of your collection (i.e. posts) and put your markdown files in there.

The next step is to create a file in the pages_directory called <name_of_your_collection>/details.hbs (i.e. posts/details.hbs).

## Development

All required tooling can be installed using [mise](https://mise.jdx.dev/) with `mise install`
