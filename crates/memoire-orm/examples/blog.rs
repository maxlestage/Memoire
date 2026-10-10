//! Relations: users, posts and comments with joins, eager loading and reports.
//!
//! ```bash
//! cargo run -p memoire-orm --example blog
//! ```

use std::collections::HashMap;

use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "authors")]
pub struct Author {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "articles")]
pub struct Article {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "authors(id)", on_delete = "cascade", index)]
    pub author_id: i64,
    pub title: String,
    pub views: i32,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "comments")]
pub struct Comment {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "articles(id)", on_delete = "cascade", index)]
    pub article_id: i64,
    pub body: String,
}

#[derive(FromRow, Debug)]
pub struct AuthorStats {
    pub author_id: i64,
    pub articles: i64,
    pub views: Option<i64>,
}

fn main() -> Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        Migrator::new()
            .add(Migration::new(1, "blog").create_table::<Author>().create_table::<Article>().create_table::<Comment>())
            .run(&db)
            .await?;

        let mut ids = Vec::new();
        for name in ["Ada", "Grace", "Linus"] {
            let mut a = Author { id: 0, name: name.into() };
            a.insert(&db).await?;
            ids.push(a.id);
        }
        for (author, title, views) in
            [(ids[0], "Notes on the Engine", 1200), (ids[0], "Loops", 300), (ids[1], "Compilers", 950)]
        {
            let mut article = Article { id: 0, author_id: author, title: title.into(), views };
            article.insert(&db).await?;
            for i in 0..(views / 400) {
                Comment { id: 0, article_id: article.id, body: format!("comment #{i}") }.insert(&db).await?;
            }
        }

        // INNER JOIN: (Article, Author) pairs.
        let rows: Vec<(Article, Author)> = Article::query()
            .inner_join::<Author>(Author::ID.eq_col(Article::AUTHOR_ID))
            .order_by(Article::VIEWS.desc())
            .all(&db)
            .await?;
        for (article, author) in &rows {
            println!("{:<22} by {:<6} {:>5} views", article.title, author.name, article.views);
        }

        // LEFT JOIN: authors without articles show up with `None`.
        let rows: Vec<(Author, Option<Article>)> = Author::query()
            .left_join::<Article>(Article::AUTHOR_ID.eq_col(Author::ID))
            .filter(Article::ID.is_null())
            .all(&db)
            .await?;
        for (author, _) in rows {
            println!("{} has not written anything yet", author.name);
        }

        // Eager loading without N+1 queries: one query for all comments.
        let articles = Article::all(&db).await?;
        let comments =
            Comment::query().filter(Comment::ARTICLE_ID.is_in(articles.iter().map(|a| a.id))).all(&db).await?;
        let mut by_article: HashMap<i64, Vec<Comment>> = HashMap::new();
        for c in comments {
            by_article.entry(c.article_id).or_default().push(c);
        }
        for a in &articles {
            println!("{:<22} {} comments", a.title, by_article.get(&a.id).map_or(0, Vec::len));
        }

        // GROUP BY report.
        let stats: Vec<AuthorStats> = Article::query()
            .group_by(Article::AUTHOR_ID)
            .select_exprs([Article::AUTHOR_ID.expr(), count_all(), Article::VIEWS.sum()])
            .order_by(Article::AUTHOR_ID.asc())
            .all(&db)
            .await?;
        println!("{stats:#?}");
        Ok(())
    })
}
